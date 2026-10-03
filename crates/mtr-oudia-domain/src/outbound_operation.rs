use crate::{
    ByteReplacement, ByteReplacementKind, EkiJikokuPatchError, GeneratedTimetable, LineEnding,
    OperationPolicy, OudiaDirection, OudiaPatch, OudiaRouteTemplate, OudiaSource, OutboundRuntime,
    ServiceTimeMillis, SourceRange, build_eki_jikoku_patch_with_groups,
};

/// 既存時刻patchを維持し、明示ON時だけ出区patchを合成する。
pub fn build_conversion_patch(
    source: &OudiaSource,
    template: &OudiaRouteTemplate,
    timetable: &GeneratedTimetable,
    station_slot_groups: &[Vec<usize>],
    operation_policy: OperationPolicy,
    outbound_runtime: Option<OutboundRuntime>,
) -> Result<OudiaPatch, EkiJikokuPatchError> {
    let base = build_eki_jikoku_patch_with_groups(
        source,
        template,
        timetable,
        station_slot_groups,
        operation_policy,
    )?;
    let Some(runtime) = outbound_runtime else {
        return Ok(base);
    };
    let first_slot = station_slot_groups
        .first()
        .and_then(|slots| slots.first())
        .copied()
        .ok_or(EkiJikokuPatchError::TargetNotFound)?;
    let first_seconds = timetable
        .stops
        .first()
        .and_then(|s| s.rounded_departure_seconds)
        .ok_or(EkiJikokuPatchError::TimeShapeMismatch)?;
    let first_time = ServiceTimeMillis::new(
        first_seconds
            .checked_mul(1000)
            .ok_or(EkiJikokuPatchError::TimeShapeMismatch)?,
    )
    .map_err(|_| EkiJikokuPatchError::TimeShapeMismatch)?;
    let out = runtime
        .outbound_time(first_time)
        .map_err(|_| EkiJikokuPatchError::TimeShapeMismatch)?;
    let out_seconds = out
        .rounded_seconds()
        .map_err(|_| EkiJikokuPatchError::TimeShapeMismatch)?
        .rem_euclid(86_400);
    let time = format!(
        "{:02}{:02}{:02}",
        out_seconds / 3600,
        out_seconds / 60 % 60,
        out_seconds % 60
    );
    let train = source
        .document
        .diagrams
        .get(template.diagram_index)
        .and_then(|d| d.trains.get(template.train_index))
        .ok_or(EkiJikokuPatchError::TargetNotFound)?;
    let index = match train.direction {
        OudiaDirection::Kudari => first_slot,
        OudiaDirection::Nobori => source
            .document
            .station_slots
            .len()
            .checked_sub(first_slot + 1)
            .ok_or(EkiJikokuPatchError::TargetNotFound)?,
    };
    let key = format!("Operation{index}B");
    let properties = source
        .document
        .properties
        .iter()
        .filter(|p| {
            p.whole_line_range.start() >= train.section_range.start()
                && p.whole_line_range.end() <= train.section_range.end()
        })
        .collect::<Vec<_>>();
    let mut target = Vec::new();
    let mut has_nested = false;
    if operation_policy != OperationPolicy::RemoveTargetTrain {
        for property in properties.iter().filter(|p| p.key.starts_with("Operation")) {
            let (root, before, nested) =
                parse_operation_key(&property.key, source.document.station_slots.len())?;
            if root == index && before {
                if nested {
                    has_nested = true;
                } else {
                    target.push(property);
                }
            }
        }
    }
    let mut replacements = base.replacements().to_vec();
    if operation_policy != OperationPolicy::RemoveTargetTrain && !target.is_empty() {
        if target.len() != 1 {
            return Err(EkiJikokuPatchError::AmbiguousOperation);
        }
        let property = target[0];
        let outbound = parse_before_operations(&property.value)?;
        if let Some((start, end)) = outbound {
            let prefix = source
                .encode_text(&property.value[..start])
                .map_err(|_| EkiJikokuPatchError::AmbiguousOperation)?
                .len();
            let length = source
                .encode_text(&property.value[start..end])
                .map_err(|_| EkiJikokuPatchError::AmbiguousOperation)?
                .len();
            let range = SourceRange::new(
                property.value_range.start() + prefix,
                property.value_range.start() + prefix + length,
            )
            .map_err(|_| EkiJikokuPatchError::AmbiguousOperation)?;
            let replacement = if property.value[start..end].contains(':') {
                format!(
                    "{:02}:{:02}:{:02}",
                    out_seconds / 3600,
                    out_seconds / 60 % 60,
                    out_seconds % 60
                )
            } else {
                time
            };
            replacements.push(operation_replacement(source, range, &replacement)?);
        } else {
            if has_nested {
                return Err(EkiJikokuPatchError::AmbiguousOperation);
            }
            let value = if property.value.is_empty() {
                format!("3/{time}$/")
            } else {
                format!("3/{time}$/,{}", property.value)
            };
            replacements.push(operation_replacement(source, property.value_range, &value)?);
        }
    } else {
        if has_nested {
            return Err(EkiJikokuPatchError::AmbiguousOperation);
        }
        let end = train.section_range.end();
        let start = end
            .checked_sub(1)
            .ok_or(EkiJikokuPatchError::TargetNotFound)?;
        if source.bytes.get(start) != Some(&b'.') {
            return Err(EkiJikokuPatchError::TargetNotFound);
        }
        let ending = match source.line_ending {
            LineEnding::Lf => "\n",
            LineEnding::CrLf => "\r\n",
        };
        replacements.push(operation_replacement(
            source,
            SourceRange::new(start, end).map_err(|_| EkiJikokuPatchError::TargetNotFound)?,
            &format!("{key}=3/{time}$/{ending}."),
        )?);
    }
    OudiaPatch::new(replacements).map_err(EkiJikokuPatchError::InvalidPatch)
}

fn operation_replacement(
    source: &OudiaSource,
    range: SourceRange,
    text: &str,
) -> Result<ByteReplacement, EkiJikokuPatchError> {
    Ok(ByteReplacement {
        range,
        expected: source.bytes[range.start()..range.end()].to_vec(),
        replacement: source
            .encode_text(text)
            .map_err(|_| EkiJikokuPatchError::AmbiguousOperation)?,
        kind: ByteReplacementKind::Operation,
    })
}

fn parse_operation_key(
    key: &str,
    station_count: usize,
) -> Result<(usize, bool, bool), EkiJikokuPatchError> {
    let error = || EkiJikokuPatchError::AmbiguousOperation;
    let mut parts = key.strip_prefix("Operation").ok_or_else(error)?.split('.');
    let parse = |part: &str| -> Result<(usize, bool), EkiJikokuPatchError> {
        let before = part.ends_with('B');
        if !before && !part.ends_with('A') {
            return Err(error());
        }
        let digits = &part[..part.len() - 1];
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(error());
        }
        Ok((digits.parse().map_err(|_| error())?, before))
    };
    let (root, before) = parse(parts.next().ok_or_else(error)?)?;
    if root >= station_count {
        return Err(error());
    }
    let mut nested = false;
    for part in parts {
        parse(part)?;
        nested = true;
    }
    Ok((root, before, nested))
}

/// 時刻以外は変更しない。未知作業や開始作業の競合はfail closed。
fn parse_before_operations(value: &str) -> Result<Option<(usize, usize)>, EkiJikokuPatchError> {
    if value.is_empty() {
        return Ok(None);
    }
    let error = || EkiJikokuPatchError::AmbiguousOperation;
    let mut outbound = None;
    let mut offset = 0;
    for operation in value.split(',') {
        let (kind, fields) = operation.split_once('/').ok_or_else(error)?;
        match kind {
            "3" => {
                if outbound.is_some() {
                    return Err(error());
                }
                let (time, suffix) = fields.split_once('$').ok_or_else(error)?;
                crate::oudia::parse_time(time).map_err(|_| error())?;
                let (link, numbers) = suffix.split_once('/').ok_or_else(error)?;
                if link.contains('$') || numbers.contains(['$', '/']) {
                    return Err(error());
                }
                outbound = Some((offset + 2, offset + 2 + time.len()));
            }
            "0" => {
                let fields = fields.split('$').collect::<Vec<_>>();
                if fields.len() != 3
                    || fields[0].parse::<usize>().is_err()
                    || !matches!(fields[2], "0" | "1")
                {
                    return Err(error());
                }
                let (departure, arrival) = fields[1].split_once('/').ok_or_else(error)?;
                crate::oudia::parse_time(departure).map_err(|_| error())?;
                crate::oudia::parse_time(arrival).map_err(|_| error())?;
            }
            "1" => {
                let (position, time) = fields.split_once('$').ok_or_else(error)?;
                if !matches!(position, "0" | "1") {
                    return Err(error());
                }
                crate::oudia::parse_time(time).map_err(|_| error())?;
            }
            "2" => {
                let (position, rest) = fields.split_once('$').ok_or_else(error)?;
                let (count, time) = rest.split_once('/').ok_or_else(error)?;
                if !matches!(position, "0" | "1" | "2")
                    || !count.parse::<u8>().is_ok_and(|n| (1..=10).contains(&n))
                {
                    return Err(error());
                }
                crate::oudia::parse_time(time).map_err(|_| error())?;
            }
            "6" if !fields.is_empty() && !fields.contains(['$', '/']) => {}
            _ => return Err(error()),
        }
        offset += operation.len() + 1;
    }
    Ok(outbound)
}

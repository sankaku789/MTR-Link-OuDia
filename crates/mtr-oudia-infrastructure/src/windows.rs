use crate::WindowsListeningPortProvider;
use mtr_oudia_application::{ApplicationError, ListeningPortProvider};
use std::collections::BTreeSet;
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const TCP_LISTEN: u32 = 2;
const IPV4_ROW_SIZE: usize = 24;
const IPV6_ROW_SIZE: usize = 56;

impl ListeningPortProvider for WindowsListeningPortProvider {
    fn listening_tcp_ports(&self) -> Result<Vec<u16>, ApplicationError> {
        let mut ports = BTreeSet::new();
        ports.extend(listening_ports_for(AF_INET.0 as u32, false)?);
        ports.extend(listening_ports_for(AF_INET6.0 as u32, true)?);
        Ok(ports.into_iter().collect())
    }
}

fn listening_ports_for(address_family: u32, ipv6: bool) -> Result<Vec<u16>, ApplicationError> {
    let mut size = 0_u32;
    // 安全な Vec を確保するため、最初の API 呼出は必要サイズ取得だけに限定する。
    let result = unsafe {
        GetExtendedTcpTable(
            None,
            &mut size,
            false,
            address_family,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if result != ERROR_INSUFFICIENT_BUFFER {
        return Err(api_error("GetExtendedTcpTable(size)", result));
    }
    let size = usize::try_from(size).map_err(|_| ApplicationError::ListeningPortEnumeration {
        message: "GetExtendedTcpTable returned an oversized buffer".to_owned(),
    })?;
    if size < 4 {
        return Err(ApplicationError::ListeningPortEnumeration {
            message: "GetExtendedTcpTable returned an invalid buffer size".to_owned(),
        });
    }
    let mut buffer = vec![0_u8; size];
    let mut returned_size = size as u32;
    // バッファは上で API 指定サイズを確保済みで、呼出後は純粋 Rust の境界検証へ渡す。
    let result = unsafe {
        GetExtendedTcpTable(
            Some(buffer.as_mut_ptr().cast()),
            &mut returned_size,
            false,
            address_family,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if result != 0 {
        return Err(api_error("GetExtendedTcpTable(data)", result));
    }
    parse_listening_tcp_table(&buffer, ipv6)
}

fn api_error(operation: &str, code: u32) -> ApplicationError {
    ApplicationError::ListeningPortEnumeration {
        message: format!("{operation} failed with Windows error {code}"),
    }
}

/// IP Helper の可変長テーブルを、境界を検証して loopback 到達可能な LISTEN port に変換する。
fn parse_listening_tcp_table(buffer: &[u8], ipv6: bool) -> Result<Vec<u16>, ApplicationError> {
    let count_bytes = buffer
        .get(..4)
        .ok_or_else(|| malformed_table("missing row count"))?;
    let count = u32::from_ne_bytes(count_bytes.try_into().expect("four bytes")) as usize;
    let row_size = if ipv6 { IPV6_ROW_SIZE } else { IPV4_ROW_SIZE };
    let rows_len = count
        .checked_mul(row_size)
        .ok_or_else(|| malformed_table("row count overflow"))?;
    let required = 4_usize
        .checked_add(rows_len)
        .ok_or_else(|| malformed_table("table length overflow"))?;
    if required > buffer.len() {
        return Err(malformed_table("rows exceed buffer bounds"));
    }

    let mut ports = BTreeSet::new();
    for row in buffer[4..required].chunks_exact(row_size) {
        let state = u32::from_ne_bytes(row[..4].try_into().expect("state field"));
        if state != TCP_LISTEN || !is_loopback_reachable(row, ipv6) {
            continue;
        }
        let port_offset = if ipv6 { 20 } else { 8 };
        let port = u16::from_be_bytes(row[port_offset..port_offset + 2].try_into().expect("port"));
        if port != 0 {
            ports.insert(port);
        }
    }
    Ok(ports.into_iter().collect())
}

fn is_loopback_reachable(row: &[u8], ipv6: bool) -> bool {
    if ipv6 {
        row[..16] == [0; 16] || row[..16] == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]
    } else {
        row[4..8] == [0, 0, 0, 0] || row[4..8] == [127, 0, 0, 1]
    }
}

fn malformed_table(reason: &str) -> ApplicationError {
    ApplicationError::ListeningPortEnumeration {
        message: format!("malformed GetExtendedTcpTable result: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: Vec<[u8; IPV4_ROW_SIZE]>) -> Vec<u8> {
        let mut table = (rows.len() as u32).to_ne_bytes().to_vec();
        for row in rows {
            table.extend(row);
        }
        table
    }

    fn ipv4_row(state: u32, address: [u8; 4], port: u16) -> [u8; IPV4_ROW_SIZE] {
        let mut row = [0; IPV4_ROW_SIZE];
        row[..4].copy_from_slice(&state.to_ne_bytes());
        row[4..8].copy_from_slice(&address);
        row[8..10].copy_from_slice(&port.to_be_bytes());
        row
    }

    fn ipv6_row(state: u32, address: [u8; 16], port: u16) -> [u8; IPV6_ROW_SIZE] {
        let mut row = [0; IPV6_ROW_SIZE];
        row[..16].copy_from_slice(&address);
        row[20..22].copy_from_slice(&port.to_be_bytes());
        row[48..52].copy_from_slice(&state.to_ne_bytes());
        row
    }

    #[test]
    fn parses_listen_loopback_rows_and_sorts_unique_ports() {
        let rows = vec![
            ipv4_row(TCP_LISTEN, [127, 0, 0, 1], 49182),
            ipv4_row(TCP_LISTEN, [0, 0, 0, 0], 3000),
            ipv4_row(5, [127, 0, 0, 1], 4000),
            ipv4_row(TCP_LISTEN, [127, 0, 0, 1], 49182),
            ipv4_row(TCP_LISTEN, [192, 0, 2, 1], 5000),
            ipv4_row(TCP_LISTEN, [127, 0, 0, 1], 0),
        ];
        assert_eq!(
            parse_listening_tcp_table(&table(rows), false).unwrap(),
            [3000, 49182]
        );
    }

    #[test]
    fn rejects_rows_outside_the_returned_buffer() {
        let table = 1_u32.to_ne_bytes().to_vec();
        assert!(matches!(
            parse_listening_tcp_table(&table, false),
            Err(ApplicationError::ListeningPortEnumeration { .. })
        ));
    }

    #[test]
    fn parses_ipv6_loopback_and_unspecified_rows() {
        let rows = [
            ipv6_row(
                TCP_LISTEN,
                [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
                49182,
            ),
            ipv6_row(TCP_LISTEN, [0; 16], 3000),
        ];
        let mut table = (rows.len() as u32).to_ne_bytes().to_vec();
        for row in rows {
            table.extend(row);
        }
        assert_eq!(
            parse_listening_tcp_table(&table, true).unwrap(),
            [3000, 49182]
        );
    }
}

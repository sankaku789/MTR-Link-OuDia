# Operation fixture provenance

`outbound-operation.oud2` uses the exact `Operation0B=3/2359$/1;2` line from
`DiagramEdit/manual/sample2.oud2` in the official OuDiaSecond 2.06.24 source archive:
https://oudiasecond.up.seesaa.net/image/OuDiaSecond_2.06.24.src.zip

The surrounding two-station document is a minimal synthetic test context.
The original official sample is FileType OuDiaSecond.1.17 and UTF-8 BOM.

Official `entDed/CconvCentDed.cpp` writer (BOperation_Out, lines 3327–3337)
serializes `3/<time>$<in-out link code>/<original operation numbers>`.
`DedRosenFileData/CconvCDedRosenFileData.cpp` dispatches FileType .1.16/.1.17
to this converter, not to the legacy S09 converter.

Additional exact official sample operation:
`Operation73B=3/514$/C34;G31,0/8$515/$0` (`manual/sample.oud2`).

Official older manual explains directional indexes, B/A and comma-separated lists:
https://oudiasecond.seesaa.net/article/467843165.html
Its older type-3 suffix lacks the link-code field; do not use it for new .1.16/.1.17 records.

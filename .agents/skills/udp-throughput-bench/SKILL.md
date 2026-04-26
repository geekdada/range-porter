---
name: udp-throughput-bench
description: |
  Find the maximum UDP throughput without datagram loss using iterative iperf3 binary search.
  Use when the user asks to benchmark UDP throughput, find UDP bandwidth cap, test UDP performance,
  or measure maximum loss-free UDP bitrate.
---

# UDP Throughput Benchmark

Find the maximum near-loss-free UDP bitrate on a given port using iperf3 binary search.

## Prerequisites

- `iperf3` must be installed (`which iperf3`)
- The target port must be available (no existing iperf3 server on it)

## Parameters

Collect these from the user (use defaults if not specified):

| Parameter | Default | Description |
|-----------|---------|-------------|
| `port` | 20000 | Port to run iperf3 server on |
| `host` | 127.0.0.1 | Target host |
| `streams` | 4 | Number of parallel iperf3 streams |
| `duration` | 5 | Seconds per test iteration |
| `loss_threshold` | 0.5 | Max acceptable receiver-side loss (%) |
| `search_lo` | 10 | Lower bound of bitrate search (Mbps) |
| `search_hi` | 2000 | Upper bound of bitrate search (Mbps) |

## Procedure

1. **Verify iperf3** is available. If not, tell the user to install it.

2. **Start iperf3 server** in the background on the target port:
   ```bash
   iperf3 -s -p <port> &>/dev/null &
   SERVER_PID=$!
   ```
   Register a cleanup trap: `kill $SERVER_PID; wait $SERVER_PID`

3. **Binary search** for the maximum bitrate with receiver-side loss <= threshold:
   - Set `LO=<search_lo>`, `HI=<search_hi>`, `BEST=0`
   - Loop while `HI - LO > 10`:
     - `MID = (LO + HI) / 2`
     - Run: `iperf3 -c <host> -p <port> -u -b ${MID}M -P <streams> -t <duration>`
     - Parse the **receiver SUM line** (`[SUM] ... receiver`) for loss percentage
     - The loss is in the format `lost/total (X.XX%)` at the end of the line
     - If receiver loss <= threshold: `PASS`, set `BEST=MID`, `LO=MID`
     - If receiver loss > threshold: `FAIL`, set `HI=MID`
     - Print each iteration result with bitrate, loss %, and PASS/FAIL

   **IMPORTANT**: Always check receiver-side loss, NOT sender-side loss. On loopback, sender loss is always 0% even when packets are being dropped at the receiver.

4. **Verification run**: After binary search converges, run a final verification at `BEST` Mbps and show the full iperf3 output.

5. **Report results** in a summary with:
   - Max near-loss-free bitrate (Mbps)
   - Achieved receiver throughput (Gbits/sec)
   - Actual receiver loss %
   - Jitter
   - Test configuration (streams, duration, threshold)

6. **Cleanup**: Kill the iperf3 server process.

## Output Format

Present results as a summary table:

```
=== UDP Throughput Benchmark Results ===
Target:         <host>:<port>
Streams:        <streams>
Duration:       <duration>s per iteration
Loss threshold: <threshold>%

Max loss-free bitrate: ~<BEST> Mbps
Receiver throughput:   <X.XX> Gbits/sec
Receiver loss:         <X.XX>%
Jitter:                <X.XXX> ms
```

## Notes

- On macOS loopback, the sender can push 40+ Gbps but the receiver caps around 1.5-2 Gbps due to kernel socket buffer limits. The receiver-side loss is the real bottleneck metric.
- If the initial search range doesn't bracket the cap (all PASS or all FAIL), widen the range and re-run.
- The `-u` flag is critical — it enables UDP mode. Without it, iperf3 defaults to TCP.

## History

### Commit 7ec60f2ebffad92097106e5b532c33aa3cd3ac99

Benchmark results (port 20000, 4 streams, loopback):

   Metric                │ Value
   ----------------------+---------------
   Max loss-free bitrate │ ~1,898 Mbps
   Receiver throughput   │ 7.57 Gbits/sec
   Receiver loss         │ 0.32%
   Jitter                │ 0.016 ms

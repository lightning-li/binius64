#!/usr/bin/env python3
"""
Run cargo commands with CPU and memory statistics.

This script wraps cargo commands and collects:
- CPU time (user/system)
- Peak memory usage (RSS)
- Wall clock time
"""

import argparse
import subprocess
import sys
import time
import threading
import os
import resource

# Try to import psutil for better memory tracking
try:
    import psutil
    HAS_PSUTIL = True
except ImportError:
    HAS_PSUTIL = False


def format_bytes(bytes_value: int) -> str:
    """Format bytes into human-readable string."""
    for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
        if bytes_value < 1024:
            return f"{bytes_value:.2f} {unit}"
        bytes_value /= 1024
    return f"{bytes_value:.2f} PB"


def format_time(seconds: float) -> str:
    """Format seconds into human-readable string."""
    if seconds < 60:
        return f"{seconds:.2f}s"
    minutes = int(seconds // 60)
    secs = seconds % 60
    if minutes < 60:
        return f"{minutes}m {secs:.2f}s"
    hours = minutes // 60
    mins = minutes % 60
    return f"{hours}h {mins}m {secs:.2f}s"


class MemoryMonitor:
    """Monitor memory usage of a process and its children."""

    def __init__(self, pid: int, interval: float = 0.1):
        self.pid = pid
        self.interval = interval
        self.peak_rss = 0
        self.peak_vms = 0
        self.running = False
        self._thread = None

    def _monitor_loop(self):
        """Monitor loop that tracks peak memory."""
        while self.running:
            try:
                if HAS_PSUTIL:
                    proc = psutil.Process(self.pid)
                    # Sum memory of process and all children
                    total_rss = 0
                    total_vms = 0
                    try:
                        mem = proc.memory_info()
                        total_rss = mem.rss
                        total_vms = mem.vms
                        for child in proc.children(recursive=True):
                            try:
                                child_mem = child.memory_info()
                                total_rss += child_mem.rss
                                total_vms += child_mem.vms
                            except (psutil.NoSuchProcess, psutil.AccessDenied):
                                pass
                    except (psutil.NoSuchProcess, psutil.AccessDenied):
                        pass

                    self.peak_rss = max(self.peak_rss, total_rss)
                    self.peak_vms = max(self.peak_vms, total_vms)
            except Exception:
                pass
            time.sleep(self.interval)

    def start(self):
        """Start monitoring."""
        self.running = True
        self._thread = threading.Thread(target=self._monitor_loop, daemon=True)
        self._thread.start()

    def stop(self):
        """Stop monitoring."""
        self.running = False
        if self._thread:
            self._thread.join(timeout=1.0)


def run_with_stats(cmd: list, verbose: bool = True) -> dict:
    """
    Run a command and collect CPU/memory statistics.

    Returns a dict with:
    - exit_code: Process exit code
    - wall_time: Wall clock time in seconds
    - user_time: User CPU time in seconds
    - sys_time: System CPU time in seconds
    - peak_rss: Peak resident set size in bytes
    - peak_vms: Peak virtual memory size in bytes
    """
    if verbose:
        print(f"Running: {' '.join(cmd)}")
        print("-" * 60)

    # Record start time and resources
    start_wall = time.time()
    start_resource = resource.getrusage(resource.RUSAGE_CHILDREN)

    # Start the process
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )

    # Start memory monitoring if psutil is available
    monitor = None
    if HAS_PSUTIL:
        monitor = MemoryMonitor(proc.pid)
        monitor.start()

    # Stream output in real-time
    output_lines = []
    try:
        for line in proc.stdout:
            if verbose:
                print(line, end='')
            output_lines.append(line)
    except Exception:
        pass

    # Wait for process to complete
    proc.wait()

    # Stop memory monitoring
    if monitor:
        monitor.stop()

    # Record end time and resources
    end_wall = time.time()
    end_resource = resource.getrusage(resource.RUSAGE_CHILDREN)

    # Calculate statistics
    wall_time = end_wall - start_wall
    user_time = end_resource.ru_utime - start_resource.ru_utime
    sys_time = end_resource.ru_stime - start_resource.ru_stime

    # Get peak memory from resource module (in KB on macOS, bytes on Linux)
    # ru_maxrss is in bytes on macOS, kilobytes on Linux
    if sys.platform == 'darwin':
        resource_peak_rss = end_resource.ru_maxrss  # Already in bytes on macOS
    else:
        resource_peak_rss = end_resource.ru_maxrss * 1024  # Convert KB to bytes on Linux

    # Use psutil peak if available and larger
    peak_rss = resource_peak_rss
    peak_vms = 0
    if monitor:
        peak_rss = max(peak_rss, monitor.peak_rss)
        peak_vms = monitor.peak_vms

    stats = {
        'exit_code': proc.returncode,
        'wall_time': wall_time,
        'user_time': user_time,
        'sys_time': sys_time,
        'cpu_time': user_time + sys_time,
        'peak_rss': peak_rss,
        'peak_vms': peak_vms,
        'output': ''.join(output_lines),
    }

    return stats


def print_stats(stats: dict):
    """Print statistics in a formatted way."""
    print("\n" + "=" * 60)
    print("RESOURCE STATISTICS")
    print("=" * 60)

    print(f"\n{'Time Statistics':}")
    print(f"  Wall clock time:    {format_time(stats['wall_time'])}")
    print(f"  User CPU time:      {format_time(stats['user_time'])}")
    print(f"  System CPU time:    {format_time(stats['sys_time'])}")
    print(f"  Total CPU time:     {format_time(stats['cpu_time'])}")

    if stats['wall_time'] > 0:
        cpu_utilization = (stats['cpu_time'] / stats['wall_time']) * 100
        print(f"  CPU utilization:    {cpu_utilization:.1f}%")

    print(f"\n{'Memory Statistics':}")
    print(f"  Peak RSS:           {format_bytes(stats['peak_rss'])}")
    if stats['peak_vms'] > 0:
        print(f"  Peak Virtual:       {format_bytes(stats['peak_vms'])}")

    if not HAS_PSUTIL:
        print("\n  Note: Install 'psutil' for more accurate memory tracking:")
        print("        pip install psutil")

    print("\n" + "=" * 60)
    print(f"Exit code: {stats['exit_code']}")
    print("=" * 60)


def main():
    parser = argparse.ArgumentParser(
        description="Run cargo commands with CPU and memory statistics"
    )
    parser.add_argument(
        "--example", "-e",
        default="keccak_merkle_path",
        help="Example name (default: keccak_merkle_path)"
    )
    parser.add_argument(
        "--max-depth", "-d",
        type=int,
        default=21538,
        help="Max depth parameter (default: 21538)"
    )
    parser.add_argument(
        "--command", "-c",
        choices=["prove", "verify", "stat"],
        default="prove",
        help="Command to run (default: prove)"
    )
    parser.add_argument(
        "--release", "-r",
        action="store_true",
        help="Build in release mode"
    )
    parser.add_argument(
        "--quiet", "-q",
        action="store_true",
        help="Suppress command output"
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output statistics as JSON"
    )
    parser.add_argument(
        "extra_args",
        nargs="*",
        help="Additional arguments to pass to the command"
    )

    args = parser.parse_args()

    # Build the cargo command
    cmd = [
        "cargo", "run",
        "-p", "binius-examples",
        "--example", args.example,
    ]

    if args.release:
        cmd.append("--release")

    cmd.extend([
        "--",
        args.command,
        "--max-depth", str(args.max_depth),
    ])

    # Add any extra arguments
    cmd.extend(args.extra_args)

    # Run with statistics
    stats = run_with_stats(cmd, verbose=not args.quiet)

    # Print statistics
    if args.json:
        import json
        # Remove output from JSON (too large)
        json_stats = {k: v for k, v in stats.items() if k != 'output'}
        print(json.dumps(json_stats, indent=2))
    else:
        print_stats(stats)

    sys.exit(stats['exit_code'])


if __name__ == "__main__":
    main()

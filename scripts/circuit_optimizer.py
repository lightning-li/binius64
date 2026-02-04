#!/usr/bin/env python3
"""
Binius Circuit Optimizer

This tool helps determine optimal circuit parameters to achieve balanced performance
by analyzing padding boundaries and resource utilization.

In Binius, both constraints and witness values are padded to powers of 2.
Performance changes significantly when crossing these boundaries.
"""

import argparse
import subprocess
import re
import json
from dataclasses import dataclass
from typing import Optional, Tuple, List
import math


@dataclass
class CircuitStats:
    """Statistics for a circuit at a given parameter value."""
    param_value: int
    and_constraints: int
    and_padded: int  # Power of 2
    mul_constraints: int
    mul_padded: int
    committed_total: int
    committed_padded: int  # Power of 2

    @property
    def and_utilization(self) -> float:
        return self.and_constraints / self.and_padded * 100

    @property
    def committed_utilization(self) -> float:
        return self.committed_total / self.committed_padded * 100


def next_power_of_2(n: int) -> int:
    """Return the smallest power of 2 >= n."""
    if n <= 0:
        return 1
    return 1 << (n - 1).bit_length()


def log2(n: int) -> int:
    """Return log2 of a power of 2."""
    return n.bit_length() - 1


def parse_stat_output(output: str, param_value: int) -> Optional[CircuitStats]:
    """Parse the output of 'stat' command."""
    # Parse AND constraints: "├─ AND constraints: 66,064 used (50.4% of 2^17)"
    and_match = re.search(r'AND constraints: ([\d,]+) used.*2\^(\d+)', output)
    if not and_match:
        return None
    and_constraints = int(and_match.group(1).replace(',', ''))
    and_log2 = int(and_match.group(2))

    # Parse MUL constraints
    mul_match = re.search(r'MUL constraints: ([\d,]+) used.*2\^(\d+)', output)
    mul_constraints = int(mul_match.group(1).replace(',', '')) if mul_match else 0
    mul_log2 = int(mul_match.group(2)) if mul_match else 0

    # Parse Total Committed: "├─ Total Committed: 66,632 used (50.8% of 2^17)"
    committed_match = re.search(r'Total Committed: ([\d,]+) used.*2\^(\d+)', output)
    if not committed_match:
        return None
    committed_total = int(committed_match.group(1).replace(',', ''))
    committed_log2 = int(committed_match.group(2))

    return CircuitStats(
        param_value=param_value,
        and_constraints=and_constraints,
        and_padded=1 << and_log2,
        mul_constraints=mul_constraints,
        mul_padded=1 << mul_log2 if mul_log2 > 0 else 1,
        committed_total=committed_total,
        committed_padded=1 << committed_log2,
    )


def run_stat_command(example: str, param_name: str, param_value: int) -> Optional[CircuitStats]:
    """Run the stat command for a given parameter value."""
    cmd = [
        'cargo', 'run', '-p', 'binius-examples',
        '--example', example, '--',
        'stat', f'--{param_name}', str(param_value)
    ]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
        return parse_stat_output(result.stdout, param_value)
    except subprocess.TimeoutExpired:
        return None
    except Exception as e:
        print(f"Error running command: {e}")
        return None


def binary_search_boundary(
    example: str,
    param_name: str,
    low: int,
    high: int,
    target_log2: int,
    check_and: bool = True
) -> int:
    """Binary search to find the boundary where padding level changes."""
    while low < high:
        mid = (low + high + 1) // 2
        stats = run_stat_command(example, param_name, mid)
        if stats is None:
            high = mid - 1
            continue

        current_log2 = log2(stats.and_padded if check_and else stats.committed_padded)
        if current_log2 <= target_log2:
            low = mid
        else:
            high = mid - 1

    return low


def estimate_linear_relationship(stats_list: List[CircuitStats]) -> Tuple[float, float, float, float]:
    """Estimate linear coefficients: constraints = a * param + b, witness = c * param + d"""
    if len(stats_list) < 2:
        return 0, 0, 0, 0

    # Simple linear regression
    x = [s.param_value for s in stats_list]
    y_and = [s.and_constraints for s in stats_list]
    y_wit = [s.committed_total for s in stats_list]

    n = len(x)
    sum_x = sum(x)
    sum_x2 = sum(xi**2 for xi in x)

    # AND constraints
    sum_y_and = sum(y_and)
    sum_xy_and = sum(xi * yi for xi, yi in zip(x, y_and))
    a = (n * sum_xy_and - sum_x * sum_y_and) / (n * sum_x2 - sum_x**2)
    b = (sum_y_and - a * sum_x) / n

    # Witness
    sum_y_wit = sum(y_wit)
    sum_xy_wit = sum(xi * yi for xi, yi in zip(x, y_wit))
    c = (n * sum_xy_wit - sum_x * sum_y_wit) / (n * sum_x2 - sum_x**2)
    d = (sum_y_wit - c * sum_x) / n

    return a, b, c, d


def find_optimal_params(
    example: str,
    param_name: str,
    sample_points: List[int],
    target_utilization: float = 95.0
) -> dict:
    """
    Find optimal parameter values by sampling and analyzing the circuit.

    Returns a dict with:
    - linear coefficients
    - boundaries for each power of 2
    - recommended values
    """
    print(f"Sampling circuit at {len(sample_points)} points...")
    stats_list = []
    for i, val in enumerate(sample_points):
        print(f"  [{i+1}/{len(sample_points)}] Testing {param_name}={val}...", end=' ', flush=True)
        stats = run_stat_command(example, param_name, val)
        if stats:
            stats_list.append(stats)
            print(f"AND={stats.and_constraints}, Witness={stats.committed_total}")
        else:
            print("failed")

    if len(stats_list) < 2:
        return {"error": "Not enough valid samples"}

    # Estimate linear relationship
    a, b, c, d = estimate_linear_relationship(stats_list)
    print(f"\nEstimated relationships:")
    print(f"  AND constraints ≈ {a:.2f} × {param_name} + {b:.2f}")
    print(f"  Witness values  ≈ {c:.2f} × {param_name} + {d:.2f}")

    # Find boundaries for various power-of-2 levels
    boundaries = []
    for log2_level in range(18, 25):  # 2^14 to 2^21
        target = 1 << log2_level

        # Calculate param value where AND constraints reach target
        if a > 0:
            max_param_and = int((target - b) / a)
        else:
            max_param_and = float('inf')

        # Calculate param value where witness reaches target
        if c > 0:
            max_param_wit = int((target - d) / c)
        else:
            max_param_wit = float('inf')

        # The limiting factor
        max_param = min(max_param_and, max_param_wit)
        limiting = "AND" if max_param_and < max_param_wit else "Witness"

        if max_param > 0:
            boundaries.append({
                "log2": log2_level,
                "padded_size": target,
                "max_param_and": max_param_and,
                "max_param_witness": max_param_wit,
                "max_param": max_param,
                "limiting_factor": limiting,
            })

    # Calculate optimal values for different utilization targets
    recommendations = []
    for boundary in boundaries:
        if boundary["max_param"] <= 0:
            continue

        # Calculate utilization at max_param
        param = boundary["max_param"]
        est_and = a * param + b
        est_wit = c * param + d

        # Find param for target utilization
        target_and = boundary["padded_size"] * (target_utilization / 100)
        target_wit = boundary["padded_size"] * (target_utilization / 100)

        optimal_param_and = int((target_and - b) / a) if a > 0 else param
        optimal_param_wit = int((target_wit - d) / c) if c > 0 else param
        optimal_param = min(optimal_param_and, optimal_param_wit, boundary["max_param"])

        if optimal_param > 0:
            recommendations.append({
                "log2": boundary["log2"],
                "padded_size": boundary["padded_size"],
                "optimal_param": optimal_param,
                "estimated_and": int(a * optimal_param + b),
                "estimated_witness": int(c * optimal_param + d),
                "and_utilization": (a * optimal_param + b) / boundary["padded_size"] * 100,
                "witness_utilization": (c * optimal_param + d) / boundary["padded_size"] * 100,
            })

    return {
        "coefficients": {
            "and_slope": a,
            "and_intercept": b,
            "witness_slope": c,
            "witness_intercept": d,
        },
        "boundaries": boundaries,
        "recommendations": recommendations,
    }


def print_recommendations(result: dict, param_name: str):
    """Print formatted recommendations."""
    if "error" in result:
        print(f"Error: {result['error']}")
        return

    print("\n" + "=" * 80)
    print("OPTIMIZATION RESULTS")
    print("=" * 80)

    print("\n📊 Recommended Parameter Values:")
    print("-" * 80)
    print(f"{'Padding':>10} | {param_name:>12} | {'AND Constr.':>12} | {'Witness':>12} | {'AND %':>8} | {'Wit %':>8}")
    print("-" * 80)

    for rec in result["recommendations"]:
        print(f"  2^{rec['log2']:<6} | {rec['optimal_param']:>12} | "
              f"{rec['estimated_and']:>12,} | {rec['estimated_witness']:>12,} | "
              f"{rec['and_utilization']:>7.1f}% | {rec['witness_utilization']:>7.1f}%")

    print("-" * 80)

    # Find the "sweet spots" where both utilizations are high
    print("\n🎯 Sweet Spots (both AND and Witness utilization > 90%):")
    for rec in result["recommendations"]:
        if rec["and_utilization"] > 90 and rec["witness_utilization"] > 90:
            print(f"   • {param_name} = {rec['optimal_param']} "
                  f"(2^{rec['log2']} padding, "
                  f"AND: {rec['and_utilization']:.1f}%, "
                  f"Witness: {rec['witness_utilization']:.1f}%)")

    print("\n📈 Boundary Analysis (max values before padding increases):")
    for b in result["boundaries"]:
        if b["max_param"] > 0 and b["max_param"] < 10000:
            print(f"   • 2^{b['log2']:>2}: max {param_name} = {b['max_param']:>5} "
                  f"(limited by {b['limiting_factor']})")


def interactive_mode(example: str, param_name: str):
    """Interactive mode for exploring specific parameter values."""
    print(f"\nInteractive mode for {example} (parameter: {param_name})")
    print("Enter parameter values to test, or 'q' to quit.\n")

    while True:
        try:
            user_input = input(f"Enter {param_name} value (or 'q' to quit): ").strip()
            if user_input.lower() == 'q':
                break

            param_value = int(user_input)
            print(f"Running stat for {param_name}={param_value}...")
            stats = run_stat_command(example, param_name, param_value)

            if stats:
                print(f"\n  AND constraints:  {stats.and_constraints:>10,} / {stats.and_padded:>10,} "
                      f"(2^{log2(stats.and_padded)}, {stats.and_utilization:.1f}%)")
                print(f"  Witness values:   {stats.committed_total:>10,} / {stats.committed_padded:>10,} "
                      f"(2^{log2(stats.committed_padded)}, {stats.committed_utilization:.1f}%)")
                print()
            else:
                print("  Failed to get stats\n")
        except ValueError:
            print("  Invalid input, please enter a number\n")
        except KeyboardInterrupt:
            break


def main():
    parser = argparse.ArgumentParser(
        description="Binius Circuit Optimizer - Find optimal parameter values for balanced performance"
    )
    parser.add_argument(
        "--example", "-e",
        default="keccak_merkle_path",
        help="Example circuit name (default: keccak_merkle_path)"
    )
    parser.add_argument(
        "--param", "-p",
        default="max-depth",
        help="Parameter name to optimize (default: max-depth)"
    )
    parser.add_argument(
        "--samples", "-s",
        type=int,
        nargs="+",
        default=[10, 50, 100, 150, 200],
        help="Sample points for analysis (default: 10 50 100 150 200)"
    )
    parser.add_argument(
        "--utilization", "-u",
        type=float,
        default=95.0,
        help="Target utilization percentage (default: 95.0)"
    )
    parser.add_argument(
        "--interactive", "-i",
        action="store_true",
        help="Interactive mode for testing specific values"
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output results as JSON"
    )

    args = parser.parse_args()

    if args.interactive:
        interactive_mode(args.example, args.param)
    else:
        result = find_optimal_params(
            args.example,
            args.param,
            args.samples,
            args.utilization
        )

        if args.json:
            print(json.dumps(result, indent=2))
        else:
            print_recommendations(result, args.param)


if __name__ == "__main__":
    main()

// Copyright 2025 Irreducible Inc.
// Copyright 2026 The Binius Developers

//! Resource statistics collection for CPU and memory usage.
//!
//! This module provides utilities to measure and report:
//! - Wall clock time
//! - CPU time (user + system)
//! - Peak memory usage (RSS)

use peakmem_alloc::{PeakAlloc, PeakAllocTrait};
use std::alloc::System;
use std::time::Instant;

/// Global instrumented allocator for peak memory tracking.
#[global_allocator]
pub static PEAK_ALLOC: &PeakAlloc<System> = &peakmem_alloc::INSTRUMENTED_SYSTEM;

/// Resource statistics collected during execution.
#[derive(Debug, Clone, Default)]
pub struct ResourceStats {
	/// Wall clock time in seconds
	pub wall_time_secs: f64,
	/// User CPU time in seconds
	pub user_time_secs: f64,
	/// System CPU time in seconds
	pub sys_time_secs: f64,
	/// Peak memory usage in bytes
	pub peak_memory_bytes: usize,
}

/// Phase names for resource tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
	Build,
	Setup,
	Prove,
	Verify,
}

impl Phase {
	pub fn name(&self) -> &'static str {
		match self {
			Phase::Build => "Build",
			Phase::Setup => "Setup",
			Phase::Prove => "Prove",
			Phase::Verify => "Verify",
		}
	}
}

/// Statistics for multiple phases
#[derive(Debug, Clone, Default)]
pub struct PhasedResourceStats {
	pub build: ResourceStats,
	pub setup: ResourceStats,
	pub prove: ResourceStats,
	pub verify: ResourceStats,
	pub total: ResourceStats,
}

impl PhasedResourceStats {
	pub fn get(&self, phase: Phase) -> &ResourceStats {
		match phase {
			Phase::Build => &self.build,
			Phase::Setup => &self.setup,
			Phase::Prove => &self.prove,
			Phase::Verify => &self.verify,
		}
	}

	pub fn get_mut(&mut self, phase: Phase) -> &mut ResourceStats {
		match phase {
			Phase::Build => &mut self.build,
			Phase::Setup => &mut self.setup,
			Phase::Prove => &mut self.prove,
			Phase::Verify => &mut self.verify,
		}
	}
}

impl std::fmt::Display for PhasedResourceStats {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		writeln!(
			f,
			"╔════════════════════════════════════════════════════════════════════════════════╗"
		)?;
		writeln!(
			f,
			"║                           RESOURCE STATISTICS                                  ║"
		)?;
		writeln!(
			f,
			"╠════════════════════════════════════════════════════════════════════════════════╣"
		)?;
		writeln!(
			f,
			"║  Phase   │   Wall Time   │   CPU Time    │  CPU Util  │    Peak Memory        ║"
		)?;
		writeln!(
			f,
			"╠══════════╪═══════════════╪═══════════════╪════════════╪═══════════════════════╣"
		)?;

		for phase in [Phase::Build, Phase::Setup, Phase::Prove, Phase::Verify] {
			let stats = self.get(phase);
			writeln!(
				f,
				"║ {:<8} │ {:>13} │ {:>13} │ {:>9.1}% │ {:>21} ║",
				phase.name(),
				stats.format_wall_time(),
				stats.format_cpu_time(),
				stats.cpu_utilization(),
				stats.format_memory()
			)?;
		}

		writeln!(
			f,
			"╠══════════╪═══════════════╪═══════════════╪════════════╪═══════════════════════╣"
		)?;
		writeln!(
			f,
			"║ {:<8} │ {:>13} │ {:>13} │ {:>9.1}% │ {:>21} ║",
			"Total",
			self.total.format_wall_time(),
			self.total.format_cpu_time(),
			self.total.cpu_utilization(),
			self.total.format_memory()
		)?;
		writeln!(
			f,
			"╚════════════════════════════════════════════════════════════════════════════════╝"
		)
	}
}

impl ResourceStats {
	/// Get total CPU time (user + system).
	pub fn total_cpu_time_secs(&self) -> f64 {
		self.user_time_secs + self.sys_time_secs
	}

	/// Get CPU utilization as a percentage.
	pub fn cpu_utilization(&self) -> f64 {
		if self.wall_time_secs > 0.0 {
			(self.total_cpu_time_secs() / self.wall_time_secs) * 100.0
		} else {
			0.0
		}
	}

	/// Format peak memory as human-readable string.
	pub fn format_memory(&self) -> String {
		format_bytes(self.peak_memory_bytes)
	}

	/// Format wall time as human-readable string.
	pub fn format_wall_time(&self) -> String {
		format_duration(self.wall_time_secs)
	}

	/// Format CPU time as human-readable string.
	pub fn format_cpu_time(&self) -> String {
		format_duration(self.total_cpu_time_secs())
	}
}

impl std::fmt::Display for ResourceStats {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		writeln!(f, "╔══════════════════════════════════════════════════════════╗")?;
		writeln!(f, "║              RESOURCE STATISTICS                         ║")?;
		writeln!(f, "╠══════════════════════════════════════════════════════════╣")?;
		writeln!(f, "║ Time Statistics:                                         ║")?;
		writeln!(f, "║   Wall clock time:    {:>32} ║", self.format_wall_time())?;
		writeln!(
			f,
			"║   User CPU time:      {:>32} ║",
			format_duration(self.user_time_secs)
		)?;
		writeln!(
			f,
			"║   System CPU time:    {:>32} ║",
			format_duration(self.sys_time_secs)
		)?;
		writeln!(
			f,
			"║   Total CPU time:     {:>32} ║",
			self.format_cpu_time()
		)?;
		writeln!(
			f,
			"║   CPU utilization:    {:>31.1}% ║",
			self.cpu_utilization()
		)?;
		writeln!(f, "╠══════════════════════════════════════════════════════════╣")?;
		writeln!(f, "║ Memory Statistics:                                       ║")?;
		writeln!(
			f,
			"║   Peak memory (RSS):  {:>32} ║",
			self.format_memory()
		)?;
		writeln!(f, "╚══════════════════════════════════════════════════════════╝")
	}
}

/// A guard that tracks resource usage from creation until drop.
pub struct ResourceTracker {
	start_wall: Instant,
	start_user_time: f64,
	start_sys_time: f64,
}

impl ResourceTracker {
	/// Start tracking resources.
	pub fn new() -> Self {
		// Reset peak memory counter
		PEAK_ALLOC.reset_peak_memory();

		let (user, sys) = get_cpu_times();
		Self {
			start_wall: Instant::now(),
			start_user_time: user,
			start_sys_time: sys,
		}
	}

	/// Finish tracking and return the statistics.
	pub fn finish(self) -> ResourceStats {
		let wall_time = self.start_wall.elapsed();
		let (end_user, end_sys) = get_cpu_times();
		let peak_memory = PEAK_ALLOC.get_peak_memory();

		ResourceStats {
			wall_time_secs: wall_time.as_secs_f64(),
			user_time_secs: end_user - self.start_user_time,
			sys_time_secs: end_sys - self.start_sys_time,
			peak_memory_bytes: peak_memory,
		}
	}
}

impl Default for ResourceTracker {
	fn default() -> Self {
		Self::new()
	}
}

/// A tracker for multiple phases of execution.
pub struct PhasedResourceTracker {
	stats: PhasedResourceStats,
	total_start_wall: Instant,
	total_start_user: f64,
	total_start_sys: f64,
	current_phase: Option<Phase>,
	phase_start_wall: Option<Instant>,
	phase_start_user: f64,
	phase_start_sys: f64,
	phase_start_memory: usize,
}

impl PhasedResourceTracker {
	/// Create a new phased resource tracker.
	pub fn new() -> Self {
		PEAK_ALLOC.reset_peak_memory();
		let (user, sys) = get_cpu_times();
		Self {
			stats: PhasedResourceStats::default(),
			total_start_wall: Instant::now(),
			total_start_user: user,
			total_start_sys: sys,
			current_phase: None,
			phase_start_wall: None,
			phase_start_user: 0.0,
			phase_start_sys: 0.0,
			phase_start_memory: 0,
		}
	}

	/// Start tracking a new phase. If another phase is in progress, it will be ended first.
	pub fn start_phase(&mut self, phase: Phase) {
		// End current phase if any
		if self.current_phase.is_some() {
			self.end_phase();
		}

		// Reset peak memory for this phase
		PEAK_ALLOC.reset_peak_memory();
		self.phase_start_memory = 0;

		let (user, sys) = get_cpu_times();
		self.current_phase = Some(phase);
		self.phase_start_wall = Some(Instant::now());
		self.phase_start_user = user;
		self.phase_start_sys = sys;
	}

	/// End the current phase and record its statistics.
	pub fn end_phase(&mut self) {
		if let Some(phase) = self.current_phase.take() {
			let wall_time = self
				.phase_start_wall
				.take()
				.map(|s| s.elapsed().as_secs_f64())
				.unwrap_or(0.0);
			let (end_user, end_sys) = get_cpu_times();
			let peak_memory = PEAK_ALLOC.get_peak_memory();

			let phase_stats = self.stats.get_mut(phase);
			phase_stats.wall_time_secs = wall_time;
			phase_stats.user_time_secs = end_user - self.phase_start_user;
			phase_stats.sys_time_secs = end_sys - self.phase_start_sys;
			phase_stats.peak_memory_bytes = peak_memory;
		}
	}

	/// Finish tracking all phases and return the complete statistics.
	pub fn finish(mut self) -> PhasedResourceStats {
		// End current phase if any
		self.end_phase();

		// Calculate totals
		let (end_user, end_sys) = get_cpu_times();
		self.stats.total.wall_time_secs = self.total_start_wall.elapsed().as_secs_f64();
		self.stats.total.user_time_secs = end_user - self.total_start_user;
		self.stats.total.sys_time_secs = end_sys - self.total_start_sys;

		// Total peak memory is the max of all phases
		self.stats.total.peak_memory_bytes = [
			self.stats.build.peak_memory_bytes,
			self.stats.setup.peak_memory_bytes,
			self.stats.prove.peak_memory_bytes,
			self.stats.verify.peak_memory_bytes,
		]
		.into_iter()
		.max()
		.unwrap_or(0);

		self.stats
	}
}

impl Default for PhasedResourceTracker {
	fn default() -> Self {
		Self::new()
	}
}

/// Get current process CPU times (user, system) in seconds.
#[cfg(unix)]
fn get_cpu_times() -> (f64, f64) {
	use std::mem::MaybeUninit;

	let mut usage = MaybeUninit::<libc::rusage>::uninit();
	// SAFETY: getrusage is safe to call with a valid pointer
	let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };

	if ret == 0 {
		// SAFETY: getrusage succeeded, so usage is initialized
		let usage = unsafe { usage.assume_init() };
		let user_secs =
			usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1_000_000.0;
		let sys_secs = usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1_000_000.0;
		(user_secs, sys_secs)
	} else {
		(0.0, 0.0)
	}
}

#[cfg(not(unix))]
fn get_cpu_times() -> (f64, f64) {
	// On non-Unix platforms, fall back to wall time only
	(0.0, 0.0)
}

/// Format bytes as human-readable string.
fn format_bytes(bytes: usize) -> String {
	const KB: usize = 1024;
	const MB: usize = KB * 1024;
	const GB: usize = MB * 1024;

	if bytes >= GB {
		format!("{:.2} GB", bytes as f64 / GB as f64)
	} else if bytes >= MB {
		format!("{:.2} MB", bytes as f64 / MB as f64)
	} else if bytes >= KB {
		format!("{:.2} KB", bytes as f64 / KB as f64)
	} else {
		format!("{} B", bytes)
	}
}

/// Format duration as human-readable string.
fn format_duration(secs: f64) -> String {
	if secs < 0.001 {
		format!("{:.2} µs", secs * 1_000_000.0)
	} else if secs < 1.0 {
		format!("{:.2} ms", secs * 1000.0)
	} else if secs < 60.0 {
		format!("{:.2} s", secs)
	} else if secs < 3600.0 {
		let mins = (secs / 60.0).floor();
		let remaining = secs % 60.0;
		format!("{}m {:.2}s", mins as u32, remaining)
	} else {
		let hours = (secs / 3600.0).floor();
		let remaining = secs % 3600.0;
		let mins = (remaining / 60.0).floor();
		let secs = remaining % 60.0;
		format!("{}h {}m {:.2}s", hours as u32, mins as u32, secs)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_format_bytes() {
		assert_eq!(format_bytes(500), "500 B");
		assert_eq!(format_bytes(1024), "1.00 KB");
		assert_eq!(format_bytes(1536), "1.50 KB");
		assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
		assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
	}

	#[test]
	fn test_format_duration() {
		assert_eq!(format_duration(0.0001), "100.00 µs");
		assert_eq!(format_duration(0.5), "500.00 ms");
		assert_eq!(format_duration(1.5), "1.50 s");
		assert_eq!(format_duration(90.0), "1m 30.00s");
		assert_eq!(format_duration(3661.0), "1h 1m 1.00s");
	}

	#[test]
	fn test_resource_tracker() {
		let tracker = ResourceTracker::new();
		// Do some work
		let _v: Vec<u8> = vec![0; 1024];
		std::thread::sleep(std::time::Duration::from_millis(10));
		let stats = tracker.finish();

		assert!(stats.wall_time_secs > 0.0);
		assert!(stats.peak_memory_bytes > 0);
	}
}

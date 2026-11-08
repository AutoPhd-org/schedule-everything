//! Multi-page PDF visualization of the odd/even schedules (port of
//! `visualizer.py`). OWNER: reports agent.

use std::path::{Path, PathBuf};

use crate::config::ScheduleConfig;
use crate::py::Dict;

/// Render the schedule visualization PDF to `output_path` (default
/// `~/Desktop/schedule_visualization.pdf`) and return the path written.
pub fn visualize(
    _config: &ScheduleConfig,
    _odd_schedule: &Dict,
    _even_schedule: &Dict,
    _output_path: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    todo!("reports agent: port visualizer.py")
}

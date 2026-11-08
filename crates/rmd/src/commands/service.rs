//! Service and config-set commands: update, stop, switch, report, edit, mode
//! (port of `commands/service.py`). OWNER: service/TUI agent.

/// Restart the background reminder service. Returns `(ok, message)`.
pub fn restart_reminder_service() -> (bool, String) {
    todo!("port commands/service.py _restart_reminder_service")
}

/// `rmd update`: pull the config repo when git-managed, then reload the service.
pub fn update_command() -> i32 {
    todo!("port commands/service.py update_command")
}

/// `rmd stop`: stop the reminder service.
pub fn stop_command() -> i32 {
    todo!("port commands/service.py stop_command")
}

/// `rmd switch <config_id>`: activate another `user_config_n` and reload.
pub fn switch_command(_config_id: &str) -> i32 {
    todo!("port commands/service.py switch_command")
}

/// `rmd report <weekly|monthly> [-d DATE] [--days N]`.
pub fn report_command(_report_type: &str, _date: Option<&str>, _days: Option<i64>) -> i32 {
    todo!("port commands/service.py report_command")
}

/// `rmd edit [settings|odd|even|deadlines|ddl|habits]`.
pub fn edit_schedule_command(_file: &str) -> i32 {
    todo!("port commands/service.py edit_schedule_command")
}

/// `rmd mode [j|p]`.
pub fn mode_command(_mode: Option<&str>) -> i32 {
    todo!("port commands/service.py mode_command")
}

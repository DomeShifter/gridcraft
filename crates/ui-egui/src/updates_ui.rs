//! One background check at startup (and on Help ▸ Check for Updates…) and the modal prompt that
//! offers the new release: Update and Restart (direct self-update), Download or Later. The flow
//! itself is `craft_update::updater::Updater`.

use craft_update::CheckResultResponse;
use craft_update::updater::{Action, Status, Updater};

use crate::SheetApp;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Runtime {
    updater: Updater,
    /// The check at start was asked for; `tick` starts it with the context.
    startup: bool,
    /// Help ▸ Check for Updates… was chosen; the next `tick` starts it.
    manual: bool,
    /// Whose releases a check asks for, and the version it compares them with: GridCraft's
    /// unless the host gave its own ([`SheetApp::set_update_app`]).
    app: &'static craft_update::app::App,
    version: &'static str,
    /// Checks started in this process (the check at start counts once), for a host's tests.
    checks_started: u32,
    started_at_start: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            updater: Updater::new(&crate::UPDATE_APP, VERSION),
            startup: false,
            manual: false,
            app: &crate::UPDATE_APP,
            version: VERSION,
            checks_started: 0,
            started_at_start: false,
        }
    }
}

/// A host app that embeds GridCraft under its own name: checks ask for `app`'s releases from its
/// own repository and compare them with `version`, never GridCraft's.
pub(crate) fn set_app(app: &mut SheetApp, update_app: &'static craft_update::app::App, version: &'static str) {
    let rt = &mut app.updates_rt;
    rt.app = update_app;
    rt.version = version;
    rt.updater = Updater::new(update_app, version);
}

/// How many checks this process has started (the check at start counts once).
pub(crate) fn checks_started(app: &SheetApp) -> u32 {
    app.updates_rt.checks_started
}

/// Ask for a check; the next frame's `tick` starts it. A manual one reports every outcome.
pub(crate) fn start(app: &mut SheetApp, manual: bool) {
    if manual {
        app.updates_rt.manual = true;
    } else {
        app.updates_rt.startup = true;
    }
}

pub(crate) fn tick(app: &mut SheetApp, ctx: &egui::Context) {
    let rt = &mut app.updates_rt;
    if std::mem::take(&mut rt.manual) {
        rt.checks_started += 1;
        rt.updater.check_now(ctx);
    } else if rt.startup {
        if !std::mem::replace(&mut rt.started_at_start, true) {
            rt.checks_started += 1;
        }
        rt.updater.check_at_start(ctx);
    }
    rt.updater.tick(ctx);
}

/// Starts the helper of a ready update; it installs once the process has exited.
pub(crate) fn start_pending_update(app: &mut SheetApp) {
    app.updates_rt.updater.start_pending_update();
}

/// Whether the check result window shows: for every status but idle (a requested check, and the
/// first start after an update, which reports that it installed or why it failed), and never
/// over the startup prompt.
fn result_window_shows(status: &Status, prompt_open: bool) -> bool {
    !prompt_open && !matches!(status, Status::Idle)
}

pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    // The close for a ready update waits until no workbook holds unsaved work.
    let clean = !app.session.documents().iter().any(|d| d.is_dirty());
    match app.updates_rt.updater.show_if(ctx, clean) {
        Action::OpenUrl(url) => {
            if let Some(open) = &app.services.open_url {
                open(&url);
            }
        }
        // Quit closes every workbook window. The install helper starts once the app exits.
        Action::Close => app.request_quit(ctx),
        Action::None => {}
    }
    let rt = &mut app.updates_rt;
    if result_window_shows(rt.updater.status(), rt.updater.prompt_open()) {
        match craft_update::CheckResultDialog::new(rt.app, rt.version).show(ctx, rt.updater.status()) {
            CheckResultResponse::Close => rt.updater.dismiss_status(),
            CheckResultResponse::CheckAgain => {
                rt.checks_started += 1;
                rt.updater.check_now(ctx);
            }
            // The next `show_if` asks the window to close once nothing is unsaved.
            CheckResultResponse::RestartToUpdate => rt.updater.restart_to_update(),
            CheckResultResponse::None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host's own identity replaces GridCraft's, and frames alone never start a check.
    #[test]
    fn a_host_update_app_is_used_and_no_check_starts_unasked() {
        static HOST: craft_update::app::App = craft_update::app::App::new("hostapp", "Host App", "A host", "example/host");
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, crate::Services::default());
        app.set_update_app(&HOST, "1.2.3");
        assert_eq!(app.updates_rt.app.repo, "example/host");
        assert_eq!(app.updates_rt.version, "1.2.3");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            tick(&mut app, &ctx);
        }
        assert_eq!(app.update_checks_started(), 0, "no check without being asked");
    }

    #[test]
    fn result_window_shows_every_status_but_idle() {
        let shown = [
            Status::Checking,
            Status::UpToDate,
            Status::Failed("no network".into()),
            Status::Installing,
            Status::Ready,
            Status::Managed,
            Status::Updated("0.2.0".into()),
            Status::InstallFailed("disk full".into()),
        ];
        for status in &shown {
            assert!(result_window_shows(status, false), "{status:?}");
            assert!(!result_window_shows(status, true), "{status:?} over the prompt");
        }
        assert!(!result_window_shows(&Status::Idle, false));
    }
}

//! Integration smoke using only the generated test fixture, never a user's
//! archive. No application is launched automatically by this test.
use super::*;

impl Window {
    pub(super) fn smoke_install_fixture(self: &Rc<Self>, parent: PathBuf) {
        assert!(!unpack_core::config::sandboxed_package());
        let (inspection, detection) = {
            let state = self.state.borrow();
            (
                state.inspection.clone().expect("Smoke archive loaded"),
                state.detection.clone().expect("Smoke detection available"),
            )
        };
        assert_eq!(
            detection.name, "Friendly Example",
            "Only generated smoke fixture may be installed"
        );
        let entry_point = detection.recommended.clone().expect("Fixture entry point");
        let paths = self.paths.clone();
        let options = installer::InstallOptions {
            id: "unpack-smoke-fixture".into(),
            name: detection.name.clone(),
            destination_parent: parent,
            entry_point,
            create_launcher: true,
            install_icon: true,
            icon_path: detection.recommended_icon.clone(),
            terminal: false,
            arguments: vec![],
            profile: "Personal".into(),
            update: false,
        };
        self.archive.id.set_text(&options.id);
        self.state
            .borrow_mut()
            .queue
            .push_back((inspection.source.clone(), "browser".into()));
        let cancel = self
            .begin("Testing fixture installation…")
            .expect("No other smoke operation");
        let limits = self.state.borrow().settings.limits.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = installer::plan(&paths, &inspection, &detection, options)
                .and_then(|plan| installer::execute(&paths, &plan, &limits, &cancel, |_| {}));
            match result {
                Ok(app) => {
                    let _ = sender.send_blocking(Event::Installed(app));
                }
                Err(error) => {
                    let _ = sender.send_blocking(Event::Failure(error.to_string()));
                }
            }
        });
    }

    pub(super) fn assert_installed_smoke(&self) {
        let app = self
            .current_installed_app()
            .expect("Fixture installation completed");
        assert_eq!(self.archive.review.label().as_deref(), Some("Open"));
        assert_eq!(
            self.archive.review.action_name().as_deref(),
            Some("win.primary")
        );
        assert_eq!(
            self.state.borrow().queue.len(),
            1,
            "Installation must not navigate away before the user can Open"
        );
        self.state.borrow_mut().queue.clear();
        self.update_queue_ui();
        let launcher = app
            .artifacts
            .iter()
            .find(|a| a.path.extension().is_some_and(|s| s == "desktop"))
            .expect("System launcher created");
        let desktop = gio::DesktopAppInfo::from_filename(&launcher.path)
            .expect("GIO can load installed desktop entry");
        let icon = desktop
            .icon()
            .expect("Installed launcher has icon")
            .downcast::<gio::FileIcon>()
            .expect("Desktop icon resolves directly to a file");
        let icon_path = icon.file().path().expect("Installed icon is local");
        assert!(icon_path.is_file());
        assert!(icon_path.starts_with(&self.paths.icons));
        assert!(app.artifacts.iter().any(|a| a.path == icon_path));
        self.archive.update.set_active(true);
        assert_eq!(self.archive.review.label().as_deref(), Some("Update…"));
        self.archive.update.set_active(false);
        assert_eq!(self.archive.review.label().as_deref(), Some("Open"));
        // Registry refreshes (including removal/undo) must not leave a stale Open
        // action. Loading the recorded app back restores Open for the same archive.
        let apps = self.state.borrow().applications.clone();
        self.state.borrow_mut().applications.clear();
        self.sync_install_action();
        assert_eq!(self.archive.review.label().as_deref(), Some("Install…"));
        self.state.borrow_mut().applications = apps;
        self.sync_install_action();
        assert_eq!(self.archive.review.label().as_deref(), Some("Open"));
        eprintln!("INSTALL SMOKE PASS: installed action is Open; update remains available; GIO resolves the system launcher's installed icon; no application was executed");
    }
}

use super::*;

impl Window {
    pub(super) fn refresh_registry(self: &Rc<Self>) {
        let paths = self.paths.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = installer::registry(&paths)
                .and_then(|apps| installer::history(&paths).map(|history| (apps, history)));
            match result {
                Ok((apps, history)) => {
                    let _ = sender.send_blocking(Event::Registry(apps, history));
                }
                Err(e) => {
                    let _ = sender.send_blocking(Event::BackgroundWarning(e.to_string()));
                }
            }
        });
    }
    pub(super) fn show_applications(self: &Rc<Self>, applications: &[Manifest]) {
        while let Some(child) = self.apps_list.first_child() {
            self.apps_list.remove(&child);
        }
        if applications.is_empty() {
            if unpack_core::config::sandboxed_package() {
                self.apps_list.append(&adw::StatusPage::builder().icon_name("application-x-executable-symbolic").title("Host installation is not available in Flatpak").description("Use the AppImage or native edition to install and manage portable applications. Archive browsing, creation and extraction work in this sandbox.").build());
                return;
            }
            let empty = adw::StatusPage::builder().icon_name("application-x-executable-symbolic").title("Your applications, all together")
                .description("Install a portable application from an archive to see it here. System packages and unmanaged applications are not listed.").build();
            self.apps_list.append(&empty);
            return;
        }
        for manifest in applications {
            let row = adw::ActionRow::builder()
                .title(&manifest.name)
                .subtitle(format!(
                    "{} · {}",
                    manifest.profile,
                    manifest.destination.display()
                ))
                .build();
            row.set_subtitle_selectable(true);
            let icon = gtk::Image::from_icon_name("application-x-executable-symbolic");
            icon.set_pixel_size(32);
            self.load_installed_icon(&icon, manifest);
            row.add_prefix(&icon);
            let launch = gtk::Button::builder()
                .icon_name("media-playback-start-symbolic")
                .tooltip_text("Launch application")
                .valign(gtk::Align::Center)
                .build();
            row.add_suffix(&launch);
            let details = gtk::Button::builder()
                .icon_name("view-more-symbolic")
                .tooltip_text("Application details and actions")
                .valign(gtk::Align::Center)
                .build();
            row.add_suffix(&details);
            let weak = Rc::downgrade(self);
            let app = manifest.clone();
            launch.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.launch(&app);
                }
            });
            let weak = Rc::downgrade(self);
            let app = manifest.clone();
            details.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.application_details(app.clone());
                }
            });
            self.apps_list.append(&row);
        }
    }
    fn launch(&self, manifest: &Manifest) {
        if unpack_core::config::sandboxed_package() {
            self.error("The Flatpak edition does not launch portable host applications. Use the native or AppImage edition.");
            return;
        }
        // Explicit user action only; no shell. Do not trust an edited desktop
        // file to supply a command. Launch the recorded executable directly.
        if !manifest.executable.starts_with(&manifest.destination) {
            self.error("The executable path is outside the recorded installation");
            return;
        }
        if manifest.terminal {
            let entry = unpack_core::desktop::DesktopEntry {
                name: manifest.name.clone(),
                generic_name: String::new(),
                comment: String::new(),
                executable: manifest.executable.to_string_lossy().into_owned(),
                arguments: manifest.arguments.clone(),
                icon: "application-x-executable".into(),
                terminal: true,
                categories: vec!["Utility".into()],
                startup_notify: true,
                startup_wm_class: None,
            };
            let launch = (|| -> Result<(), String> {
                let text = entry.render().map_err(|e| e.to_string())?;
                let keyfile = glib::KeyFile::new();
                keyfile
                    .load_from_data(&text, glib::KeyFileFlags::NONE)
                    .map_err(|e| e.to_string())?;
                let app = gio::DesktopAppInfo::from_keyfile(&keyfile)
                    .ok_or("Could not create a terminal launch configuration")?;
                app.launch(&[], Some(&host_launch_context()))
                    .map_err(|e| e.to_string())
            })();
            if let Err(error) = launch {
                self.error(&format!("Could not launch the terminal application: {error}. A desktop terminal emulator must be available."));
            }
            return;
        }
        let mut command = std::process::Command::new(&manifest.executable);
        command
            .args(&manifest.arguments)
            .current_dir(&manifest.destination);
        for (key, value) in unpack_core::desktop::child_environment() {
            if let Some(value) = value {
                command.env(key, value);
            } else {
                command.env_remove(key);
            }
        }
        match command.spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    if let Err(error) = child.wait() {
                        eprintln!("Application process wait failed: {error}");
                    }
                });
                self.notify(&format!("Launched {}", manifest.name));
            }
            Err(e) => self.error(&format!("Could not launch {}: {e}", manifest.name)),
        }
    }
    fn application_details(self: &Rc<Self>, manifest: Manifest) {
        let window = adw::Window::builder()
            .transient_for(&self.window)
            .title(&manifest.name)
            .default_width(620)
            .default_height(580)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let page = row_box();
        let hero = gtk::Box::new(gtk::Orientation::Horizontal, 16);
        let icon = gtk::Image::from_icon_name("application-x-executable");
        icon.set_pixel_size(64);
        self.load_installed_icon(&icon, &manifest);
        hero.append(&icon);
        hero.append(&label(&manifest.name, "title-1"));
        page.append(&hero);
        let information = label(&format!("ID: {}\nVersion: {}\nProfile: {}\nInstalled: {} (Unix time)\nLocation: {}\nExecutable: {}\nManaged files: {}\nSource SHA-256: {}", manifest.id, manifest.version.as_deref().unwrap_or("Not provided"), manifest.profile, manifest.installed_at, manifest.destination.display(), manifest.executable.display(), manifest.files.len(), manifest.source_sha256), "body");
        information.set_selectable(true);
        let information_section = gtk::Expander::builder()
            .label("Application Details")
            .child(&information)
            .build();
        let tools = gtk::Box::new(gtk::Orientation::Vertical, 8);
        let tools_section = gtk::Expander::builder()
            .label("Manage Application")
            .child(&tools)
            .build();
        for (name, action) in [
            ("Launch", "launch"),
            ("Open Installation Folder", "folder"),
            ("View Installation Manifest", "manifest"),
            ("Check Dependencies", "dependencies"),
            ("View Desktop Entry", "desktop"),
            ("Name & Icon from Archive…", "identity"),
            ("Update from an Archive…", "update"),
            ("Undo Last Operation", "undo"),
            ("Uninstall…", "uninstall"),
        ] {
            let button = gtk::Button::with_label(name);
            if action == "uninstall" {
                button.add_css_class("destructive-action");
            }
            let weak = Rc::downgrade(self);
            let app = manifest.clone();
            let details_window = window.clone();
            button.connect_clicked(move |_| { if let Some(ui) = weak.upgrade() { match action {
                "launch" => ui.launch(&app),
                "folder" => { let uri = gio::File::for_path(&app.destination).uri(); if let Err(e) = gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>) { ui.error(&e.to_string()); } },
                "manifest" => match serde_json::to_string_pretty(&app) { Ok(text) => ui.text_window("Installation Manifest", &text), Err(e) => ui.error(&e.to_string()) },
                "dependencies" => ui.check_dependencies(app.clone()),
                "identity" => ui.edit_identity_from_archive(app.clone()),
                "desktop" => {
                    if let Some(artifact) = app.artifacts.iter().find(|a| a.path.extension().and_then(|s| s.to_str()) == Some("desktop")) {
                        match std::fs::read_to_string(&artifact.path) { Ok(text) => ui.text_window("Desktop Entry", &text), Err(e) => ui.error(&e.to_string()) }
                    } else { ui.notify("No desktop launcher was created for this application"); }
                },
                "update" => { details_window.close(); ui.text_window("Updating an Application", &format!("Open the replacement archive.\n\nSet the application ID to: {}\nSet the destination parent to: {}\nSelect ‘Update existing installation’.\n\nReview the new entry point and installation plan before confirming. The existing version is retained as a recoverable backup. Modified or user-added files stop the update instead of being overwritten.", app.id, app.destination.parent().unwrap_or(std::path::Path::new("")).display())); ui.open_dialog(); },
                "undo" => { details_window.close(); let paths = ui.paths.clone(); let id = app.id.clone(); let sender = ui.sender.clone(); if ui.begin("Reversing previous operation…").is_some() { std::thread::spawn(move || { match installer::undo(&paths, &id) { Ok(()) => { let _ = sender.send_blocking(Event::Done("Previous operation reversed".into())); }, Err(e) => { let _ = sender.send_blocking(Event::Failure(e.to_string())); } } }); } },
                "uninstall" => { details_window.close(); ui.confirm_uninstall(app.clone()); }, _ => {},
            } } });
            if action == "launch" {
                button.add_css_class("suggested-action");
                button.add_css_class("pill");
                button.set_height_request(48);
                page.append(&button);
            } else {
                tools.append(&button);
            }
        }
        page.append(&tools_section);
        page.append(&information_section);
        toolbar.set_content(Some(&scroll(&page)));
        window.set_content(Some(&toolbar));
        window.present();
    }
    fn check_dependencies(self: &Rc<Self>, manifest: Manifest) {
        let ui = self.clone();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = (|| -> std::io::Result<Vec<u8>> {
                use std::io::Read;
                let file = std::fs::File::open(&manifest.executable)?;
                let mut bytes = Vec::new(); file.take(4 << 20).read_to_end(&mut bytes)?; Ok(bytes)
            })().map(|bytes| {
                if let Some(info) = unpack_core::compatibility::inspect(&bytes) {
                    let bundled: Vec<_> = manifest.files.iter().filter_map(|f| f.path.file_name().and_then(|s| s.to_str()).map(String::from)).collect();
                    format!("Architecture: {} ({}-bit)\nType: {}\nDynamic linker: {}\n\n{}\n\n{}\n\nGLIBC version references: {}\n\nThis is a static, best-effort check. Loader caches, RPATH, plug-ins and runtime behavior are not fully resolved. No downloaded code was executed.", info.architecture, info.bits, info.kind, info.interpreter.as_deref().unwrap_or("None detected"), info.warnings.join("\n"), unpack_core::compatibility::dependency_report(&info, &bundled).join("\n"), info.glibc_versions.join(", "))
                } else { "The entry point is not a recognized ELF executable. It may be a script. Runtime dependencies cannot be inferred reliably without executing it, which Unpack will not do.".into() }
            });
            let _ = tx.send_blocking(result);
        });
        glib::spawn_future_local(async move {
            match rx.recv().await {
                Ok(Ok(text)) => ui.text_window("Compatibility Inspection", &text),
                Ok(Err(e)) => ui.error(&e.to_string()),
                Err(e) => ui.error(&e.to_string()),
            }
        });
    }
    fn confirm_uninstall(self: &Rc<Self>, manifest: Manifest) {
        let dialog = adw::MessageDialog::builder()
            .transient_for(&self.window)
            .heading(format!("Remove {}?", manifest.name))
            .body(installer::uninstall_preview(&manifest).join("\n\n"))
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("remove", "Remove Application")]);
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        dialog.set_close_response("cancel");
        let ui = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "remove" {
                let Some(cancel) = ui.begin("Removing application…") else {
                    return;
                };
                let sender = ui.sender.clone();
                let paths = ui.paths.clone();
                let manifest = manifest.clone();
                std::thread::spawn(move || {
                    match installer::uninstall(&paths, &manifest, &cancel) {
                        Ok(_) => {
                            let _ = sender.send_blocking(Event::Done(format!(
                                "Removed {}. A recoverable backup was retained.",
                                manifest.name
                            )));
                        }
                        Err(e) => {
                            let _ = sender.send_blocking(Event::Failure(e.to_string()));
                        }
                    }
                });
            }
        });
        dialog.present();
    }
    pub(super) fn show_history(self: &Rc<Self>, events: &[installer::HistoryEvent]) {
        while let Some(child) = self.history_list.first_child() {
            self.history_list.remove(&child);
        }
        if events.is_empty() {
            self.history_list.append(
                &adw::StatusPage::builder()
                    .icon_name("document-open-recent-symbolic")
                    .title("A clean slate")
                    .description("Installation, update and removal details will appear here.")
                    .build(),
            );
            return;
        }
        for event in events.iter().rev() {
            let row = adw::ActionRow::builder()
                .title(format!("{} {}", event.action, event.application))
                .subtitle(format!("{} · {}", event.id, event.timestamp))
                .activatable(true)
                .build();
            let undo = gtk::Button::builder()
                .icon_name("edit-undo-symbolic")
                .tooltip_text("Undo the most recent retained operation for this application")
                .valign(gtk::Align::Center)
                .build();
            let weak = Rc::downgrade(self);
            let id = event.id.clone();
            undo.connect_clicked(move |_| { if let Some(ui) = weak.upgrade() {
                let dialog = adw::MessageDialog::builder().transient_for(&ui.window).heading("Reverse the most recent operation?").body("This reverses the most recent retained operation for this application, which may differ from the selected history row. Modified managed files and conflicting targets stop the reversal. Backups are retained.").build();
                dialog.add_responses(&[("cancel", "Cancel"), ("undo", "Reverse Operation")]); dialog.set_close_response("cancel");
                let ui = ui.clone(); let id = id.clone(); dialog.connect_response(None, move |_, response| { if response == "undo" && ui.begin("Reversing previous operation…").is_some() {
                    let paths = ui.paths.clone(); let id = id.clone(); let sender = ui.sender.clone();
                    std::thread::spawn(move || { match installer::undo(&paths, &id) { Ok(()) => { let _ = sender.send_blocking(Event::Done("Previous operation reversed".into())); }, Err(e) => { let _ = sender.send_blocking(Event::Failure(e.to_string())); } } });
                } }); dialog.present();
            } });
            row.add_suffix(&undo);
            let weak = Rc::downgrade(self);
            let event = event.clone();
            row.connect_activated(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.text_window("Operation Details", &event.details.join("\n\n"));
                }
            });
            self.history_list.append(&row);
        }
    }
}

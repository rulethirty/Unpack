use super::*;

impl Window {
    pub(super) fn load_installed_icon(&self, image: &gtk::Image, manifest: &Manifest) {
        let Some(artifact) = manifest
            .artifacts
            .iter()
            .find(|a| a.path.starts_with(&self.paths.icons))
        else {
            return;
        };
        let path = artifact.path.clone();
        let expected = artifact.sha256.clone();
        let image = image.downgrade();
        let (tx, rx) = async_channel::bounded(1);
        std::thread::spawn(move || {
            let result = (|| -> Result<super::icon_decode::Pixels, String> {
                use std::io::Read;
                let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                file.take(unpack_core::identity::MAX_ICON_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                unpack_core::identity::validate_icon(&path, &bytes).map_err(|e| e.to_string())?;
                if unpack_core::security::digest_reader(std::io::Cursor::new(&bytes))
                    .map_err(|e| e.to_string())?
                    != expected
                {
                    return Err("Installed icon was changed; using the default icon".into());
                }
                super::icon_decode::decode(&path, bytes)
            })();
            let _ = tx.send_blocking(result);
        });
        let sender = self.sender.clone();
        glib::spawn_future_local(async move {
            if let Ok(result) = rx.recv().await {
                match result {
                    Ok(bytes) => {
                        if let Some(image) = image.upgrade() {
                            image.set_paintable(Some(&super::icon_decode::texture(bytes)));
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Event::BackgroundWarning(error)).await;
                    }
                }
            }
        });
    }

    pub(super) fn edit_identity_from_archive(self: &Rc<Self>, manifest: Manifest) {
        if unpack_core::config::sandboxed_package() {
            self.error("Use the native or AppImage edition to change host application metadata.");
            return;
        }
        let dialog = gtk::FileDialog::builder()
            .title("Choose an Archive for the Application Name & Icon")
            .build();
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let file = match dialog.open_future(Some(&ui.window)).await {
                Ok(file) => file,
                Err(e) if e.matches(gtk::DialogError::Dismissed) => return,
                Err(e) => {
                    ui.error(&e.to_string());
                    return;
                }
            };
            let Some(path) = file.path() else {
                return;
            };
            let Some(cancel) = ui.begin("Finding the name and icon in the archive…") else {
                return;
            };
            let limits = ui.state.borrow().settings.limits.clone();
            let (tx, rx) = async_channel::bounded(1);
            std::thread::spawn(move || {
                let result = archive::inspect(&path, &limits, &cancel, |_| {});
                let _ = tx.send_blocking(result);
            });
            match rx.recv().await {
                Ok(Ok(inspection)) => {
                    ui.finish();
                    ui.confirm_identity(manifest, inspection);
                }
                Ok(Err(e)) => {
                    ui.finish();
                    ui.error(&e.to_string());
                }
                Err(e) => {
                    ui.finish();
                    ui.error(&e.to_string());
                }
            }
        });
    }

    fn confirm_identity(self: &Rc<Self>, manifest: Manifest, inspection: Inspection) {
        let detected = unpack_core::identity::detect(&inspection);
        let dialog = adw::MessageDialog::builder().transient_for(&self.window).heading("Update Name & Icon")
            .body("Use details from this archive. Your application files and user data will not change. Existing integration files are backed up for undo.").build();
        let fields = gtk::Box::new(gtk::Orientation::Vertical, 10);
        fields.append(&label("Application name", "heading"));
        let name = gtk::Entry::builder()
            .text(detected.name.as_deref().unwrap_or(&manifest.name))
            .build();
        fields.append(&name);
        fields.append(&label(
            &detected
                .name_source
                .map(|p| format!("Found in {}", p.display()))
                .unwrap_or_else(|| {
                    "No desktop name in this archive; keeping the current name.".into()
                }),
            "dim-label",
        ));
        let icons: Vec<_> = inspection
            .entries
            .iter()
            .filter(|e| unpack_core::identity::supported_icon(e))
            .map(|e| e.path.clone())
            .collect();
        let mut choices = vec!["Keep the current icon".to_string()];
        choices.extend(icons.iter().map(|p| p.display().to_string()));
        let refs: Vec<_> = choices.iter().map(String::as_str).collect();
        let choice = gtk::DropDown::from_strings(&refs);
        choice.set_enable_search(true);
        choice.set_selected(
            detected
                .icon
                .as_ref()
                .and_then(|p| icons.iter().position(|x| x == p))
                .map(|i| i as u32 + 1)
                .unwrap_or(0),
        );
        fields.append(&label("Icon from this archive", "heading"));
        fields.append(&choice);
        dialog.set_extra_child(Some(&fields));
        dialog.add_responses(&[("cancel", "Cancel"), ("apply", "Apply Changes")]);
        dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("cancel");
        let ui = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "apply" {
                let Some(cancel) = ui.begin("Updating application name and icon…") else {
                    return;
                };
                let icon = choice
                    .selected()
                    .checked_sub(1)
                    .and_then(|i| icons.get(i as usize).cloned());
                let paths = ui.paths.clone();
                let manifest = manifest.clone();
                let inspection = inspection.clone();
                let name = name.text().to_string();
                let sender = ui.sender.clone();
                std::thread::spawn(move || {
                    match installer::refresh_identity(
                        &paths,
                        &manifest,
                        &inspection,
                        &name,
                        icon.as_deref(),
                        &cancel,
                    ) {
                        Ok(updated) => {
                            let _ = sender.send_blocking(Event::MetadataUpdated(updated));
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
}

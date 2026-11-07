use super::*;
use unpack_core::{
    archive::{Entry, EntryKind, Format},
    installer::InstallOptions,
};

pub(super) struct ArchiveView {
    pub widget: gtk::Box,
    pub search: gtk::SearchEntry,
    pub list: gtk::ListBox,
    pub title: gtk::Label,
    pub subtitle: gtk::Label,
    pub findings: gtk::Label,
    pub name: gtk::Entry,
    pub id: gtk::Entry,
    pub destination: gtk::Entry,
    pub executable: gtk::DropDown,
    pub launcher: gtk::CheckButton,
    pub icon: gtk::CheckButton,
    pub terminal: gtk::CheckButton,
    pub update: gtk::CheckButton,
    pub review: gtk::Button,
    pub pages: gtk::Stack,
    pub next: gtk::Button,
    pub back: gtk::Button,
    pub app_icon: gtk::Image,
    pub app_name: gtk::Label,
    pub summary: gtk::Label,
    pub install_hint: gtk::Label,
    pub attention: gtk::Label,
    pub readme: gtk::Button,
    pub fetch: gtk::Button,
    pub metadata_source: gtk::Label,
    pub icon_choice: gtk::DropDown,
    pub archive_info: gtk::Label,
    pub identity_section: gtk::Expander,
    pub location_section: gtk::Expander,
    pub launch_section: gtk::Expander,
    pub advanced_section: gtk::Expander,
    pub inspection_section: gtk::Expander,
    pub icon_paths: RefCell<Vec<PathBuf>>,
    pub directory: RefCell<PathBuf>,
    pub entry_paths: RefCell<Vec<PathBuf>>,
    pub visible_entries: RefCell<Vec<PathBuf>>,
}
pub(super) fn size(bytes: u64) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MiB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
impl Window {
    pub(super) fn connect_archive(self: &Rc<Self>) {
        let breakpoint = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 850sp").expect("valid static breakpoint"),
        );
        breakpoint.add_setter(&self.navigation, "collapsed", Some(&true.to_value()));
        self.window.add_breakpoint(breakpoint);
        let weak = Rc::downgrade(self);
        self.archive.name.connect_changed(move |entry| {
            if let Some(ui) = weak.upgrade() {
                ui.archive.app_name.set_text(entry.text().as_str());
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.fetch.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                if let Some(detection) = ui.state.borrow().detection.as_ref() {
                    ui.archive.name.set_text(&detection.name);
                    ui.archive.id.set_text(&detection.suggested_id);
                    let selected = detection
                        .recommended_icon
                        .as_ref()
                        .and_then(|p| ui.archive.icon_paths.borrow().iter().position(|x| x == p))
                        .map(|i| i as u32 + 1)
                        .unwrap_or(0);
                    ui.archive.icon_choice.set_selected(selected);
                    ui.archive.icon.set_active(selected > 0);
                }
                ui.notify("Name and icon restored from this archive. No internet lookup was used.");
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.icon_choice.connect_selected_notify(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.refresh_archive_icon();
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.icon.connect_toggled(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.refresh_archive_icon();
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.readme.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                let state = ui.state.borrow();
                if let (Some(archive), Some(detection)) = (&state.inspection, &state.detection) {
                    let text = detection
                        .readmes
                        .iter()
                        .map(|p| {
                            format!(
                                "{}\n\n{}",
                                p.display(),
                                detection::preview_text(archive, p).unwrap_or_else(|| {
                                    "Binary or non-UTF-8 text. Inspect it in Files.".into()
                                })
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n────────\n\n");
                    ui.text_window("Instructions from the Archive", &text);
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.search.connect_search_changed(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.refresh_entries();
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.back.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.archive.directory.borrow_mut().pop();
                ui.refresh_entries();
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.list.connect_row_activated(move |_, row| {
            if let Some(ui) = weak.upgrade() {
                let path = ui.archive.visible_entries.borrow().get(row.index() as usize).cloned();
                if let Some(path) = path {
                    let state = ui.state.borrow();
                    if let Some(inspection) = &state.inspection {
                        if inspection.entries.iter().any(|e| e.path.starts_with(&path) && e.path != path) || inspection.entries.iter().any(|e| e.path == path && e.kind == EntryKind::Directory) {
                            *ui.archive.directory.borrow_mut() = path; drop(state); ui.archive.search.set_text(""); ui.refresh_entries();
                        } else { let preview = detection::preview_text(inspection, &path).unwrap_or_else(|| "Binary, link, or unsupported text encoding. Unpack never executes files from the archive browser.".into()); ui.text_window(&path.display().to_string(), &preview); }
                    }
                }
            }
        });
        let weak = Rc::downgrade(self);
        self.archive.next.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                if !ui.state.borrow().busy {
                    ui.next_queued();
                }
            }
        });
    }

    pub(super) fn show_archive(&self, inspection: &Inspection, detection: &Detection) {
        self.archive.title.set_text(
            &inspection
                .source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy(),
        );
        self.archive.subtitle.set_text(&format!(
            "{} files · {} unpacked",
            inspection.entries.len(),
            size(inspection.total_size)
        ));
        self.archive.archive_info.set_text(&format!(
            "Format: {}\nArchive: {}\nEntries: {}\nExtracted size: {}\nSHA-256: {}",
            inspection.format,
            inspection.source.display(),
            inspection.entries.len(),
            size(inspection.total_size),
            inspection.sha256
        ));
        let mut findings = format!(
            "{} · {} confidence\n\n{}",
            detection.kind.label(),
            detection.confidence,
            detection.evidence.join("\n")
        );
        for warning in &detection.warnings {
            findings.push_str(&format!("\n\n{warning}"));
        }
        for candidate in &detection.candidates {
            if let Some(elf) = &candidate.elf {
                findings.push_str(&format!(
                    "\n\n{} · {}-bit {}",
                    candidate.path.display(),
                    elf.bits,
                    elf.architecture
                ));
                for warning in &elf.warnings {
                    findings.push_str(&format!("\n{warning}"));
                }
            }
        }
        let has_duplicates =
            !installer::duplicates(detection, &self.state.borrow().applications).is_empty();
        if has_duplicates {
            findings.push_str("\n\nA similar application is already installed. Use its exact ID and destination and explicitly select Update to replace it.");
        }
        self.archive.findings.set_text(&findings);
        if unpack_core::config::sandboxed_package() {
            self.archive.findings.set_text(&format!("Flatpak edition: inspect and extract this archive here. Host application installation requires the AppImage or native edition.\n\n{findings}"));
        }
        self.archive.name.set_text(&detection.name);
        self.archive.app_name.set_text(&detection.name);
        self.archive.summary.set_text(&format!(
            "{} · {}\nFor your account. All application files stay together.",
            detection.kind.label(),
            size(inspection.total_size)
        ));
        self.archive
            .install_hint
            .set_text(if unpack_core::config::sandboxed_package() {
                "Flatpak cannot install host applications. Use the AppImage or native edition."
            } else {
                "Review every change before installing. No administrator access needed."
            });
        let attention = if detection.recommended.is_none() {
            "Choose the main application file in Launch Settings before installing."
        } else if has_duplicates {
            "A similar app is installed. Open Advanced & Updates to review update options."
        } else {
            ""
        };
        self.archive.attention.set_text(attention);
        self.archive.attention.set_visible(!attention.is_empty());
        self.archive
            .readme
            .set_visible(!detection.readmes.is_empty());
        for expander in [
            &self.archive.identity_section,
            &self.archive.location_section,
            &self.archive.advanced_section,
            &self.archive.inspection_section,
        ] {
            expander.set_expanded(false);
        }
        self.archive
            .launch_section
            .set_expanded(detection.recommended.is_none());
        self.archive.metadata_source.set_text(&detection.name_source.as_ref().map(|p| format!("Name found in {}", p.display())).unwrap_or_else(|| "No desktop name found; using the application folder or archive name. You can edit it above.".into()));
        let paths: Vec<_> = inspection
            .entries
            .iter()
            .filter(|e| unpack_core::identity::supported_icon(e))
            .map(|e| e.path.clone())
            .collect();
        let mut icon_names = vec!["Default application icon".to_string()];
        icon_names.extend(paths.iter().map(|p| p.display().to_string()));
        let refs: Vec<_> = icon_names.iter().map(String::as_str).collect();
        *self.archive.icon_paths.borrow_mut() = paths;
        self.archive
            .icon_choice
            .set_model(Some(&gtk::StringList::new(&refs)));
        let selected = detection
            .recommended_icon
            .as_ref()
            .and_then(|p| self.archive.icon_paths.borrow().iter().position(|x| x == p))
            .map(|i| i as u32 + 1)
            .unwrap_or(0);
        self.archive.icon_choice.set_selected(selected);
        self.archive.id.set_text(&detection.suggested_id);
        let state = self.state.borrow();
        self.archive.destination.set_text(
            &state
                .settings
                .installation_directory
                .as_ref()
                .unwrap_or(&self.paths.default_install)
                .display()
                .to_string(),
        );
        self.archive
            .launcher
            .set_active(state.settings.create_launcher);
        self.archive.icon.set_active(state.settings.install_icons);
        drop(state);
        self.archive.update.set_active(false);
        // Manual selection also includes regular non-executable files because ZIP
        // distributions sometimes omit Unix permission bits.
        let mut paths: Vec<_> = detection
            .candidates
            .iter()
            .map(|c| c.path.clone())
            .collect();
        paths.extend(
            inspection
                .entries
                .iter()
                .filter(|e| {
                    e.kind == EntryKind::File && !paths_contains(&detection.candidates, &e.path)
                })
                .map(|e| e.path.clone()),
        );
        let names: Vec<_> = paths
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let refs: Vec<_> = names.iter().map(String::as_str).collect();
        self.archive
            .executable
            .set_model(Some(&gtk::StringList::new(&refs)));
        let selected = detection
            .recommended
            .as_ref()
            .and_then(|r| paths.iter().position(|p| p == r))
            .map(|i| i as u32)
            .unwrap_or(gtk::INVALID_LIST_POSITION);
        self.archive.executable.set_selected(selected);
        *self.archive.entry_paths.borrow_mut() = paths;
        *self.archive.directory.borrow_mut() = PathBuf::new();
        self.archive.search.set_text("");
        // Store temporarily for the initial rendering; the caller commits state next.
        self.render_entries(inspection);
        self.archive.review.set_sensitive(true);
    }
    pub(super) fn refresh_entries(&self) {
        if let Some(inspection) = &self.state.borrow().inspection {
            self.render_entries(inspection);
        }
    }
    fn render_entries(&self, inspection: &Inspection) {
        while let Some(child) = self.archive.list.first_child() {
            self.archive.list.remove(&child);
        }
        let query = self.archive.search.text().to_lowercase();
        let directory = self.archive.directory.borrow().clone();
        let show_hidden = self.state.borrow().settings.show_hidden;
        let mut items: std::collections::BTreeMap<PathBuf, Option<&Entry>> =
            std::collections::BTreeMap::new();
        for entry in &inspection.entries {
            if !show_hidden
                && entry
                    .path
                    .iter()
                    .any(|p| p.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            if !query.is_empty() {
                if entry.path.to_string_lossy().to_lowercase().contains(&query) {
                    items.insert(entry.path.clone(), Some(entry));
                }
            } else if let Ok(relative) = entry.path.strip_prefix(&directory) {
                if let Some(first) = relative.iter().next() {
                    let path = directory.join(first);
                    let exact = path == entry.path;
                    if exact {
                        items.insert(path, Some(entry));
                    } else {
                        items.entry(path).or_insert(None);
                    }
                }
            }
        }
        self.archive
            .back
            .set_sensitive(!directory.as_os_str().is_empty());
        let mut paths = Vec::new();
        // GTK list rows are bounded for responsiveness. Search narrows large archives.
        for (path, entry) in items.into_iter().take(2000) {
            let is_dir = entry.is_none_or(|e| e.kind == EntryKind::Directory);
            let icon = if is_dir {
                "folder-symbolic"
            } else if entry
                .is_some_and(|e| matches!(e.kind, EntryKind::Symlink | EntryKind::Hardlink))
            {
                "emblem-symbolic-link-symbolic"
            } else if entry.is_some_and(|e| e.executable) {
                "application-x-executable-symbolic"
            } else {
                "text-x-generic-symbolic"
            };
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
            margins(&row, 10);
            row.append(&gtk::Image::from_icon_name(icon));
            let text = if query.is_empty() {
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            } else {
                path.display().to_string()
            };
            let name = label(&text, "body");
            name.set_wrap(false);
            name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            name.set_hexpand(true);
            row.append(&name);
            if let Some(entry) = entry {
                if entry.executable {
                    row.append(&label("Executable", "dim-label"));
                }
                if !is_dir {
                    row.append(&label(&size(entry.size), "dim-label"));
                }
            }
            self.archive.list.append(&row);
            paths.push(path);
        }
        *self.archive.visible_entries.borrow_mut() = paths;
    }

    pub(super) fn review_install(self: &Rc<Self>) {
        if unpack_core::config::sandboxed_package() {
            self.error("Portable application installation is unavailable in this Flatpak sandbox. It cannot create host launchers or launch arbitrary host applications. Use the AppImage or native edition for installation. You can still inspect, create and extract archives here.");
            return;
        }
        if self.state.borrow().busy {
            return;
        }
        let state = self.state.borrow();
        let (Some(inspection), Some(detection)) = (&state.inspection, &state.detection) else {
            drop(state);
            self.notify("Open an application archive first");
            return;
        };
        let selected = self.archive.executable.selected() as usize;
        let Some(entry_point) = self.archive.entry_paths.borrow().get(selected).cloned() else {
            drop(state);
            self.archive.pages.set_visible_child_name("install");
            self.archive.launch_section.set_expanded(true);
            self.archive.executable.grab_focus();
            self.error("Choose the main application entry point. If none is appropriate, read the README or extract the archive instead.");
            return;
        };
        let options = InstallOptions {
            id: self.archive.id.text().to_string(),
            name: self.archive.name.text().to_string(),
            destination_parent: PathBuf::from(self.archive.destination.text().as_str()),
            entry_point,
            create_launcher: self.archive.launcher.is_active(),
            install_icon: self.archive.icon.is_active() && self.selected_archive_icon().is_some(),
            icon_path: self.selected_archive_icon(),
            terminal: self.archive.terminal.is_active(),
            arguments: vec![],
            profile: "Personal".into(),
            update: self.archive.update.is_active(),
        };
        let inspection = inspection.clone();
        let detection = detection.clone();
        drop(state);
        // Planning can hash an existing installation for update validation.
        let Some(cancel) = self.begin("Preparing installation plan…") else {
            return;
        };
        let paths = self.paths.clone();
        let sender = self.sender.clone();
        let (tx, rx) = async_channel::bounded(1);
        let ui = self.clone();
        std::thread::spawn(move || {
            let result = cancel
                .check()
                .and_then(|_| installer::plan(&paths, &inspection, &detection, options));
            let _ = tx.send_blocking(result);
        });
        glib::spawn_future_local(async move {
            match rx.recv().await {
                Ok(Ok(plan)) => {
                    ui.finish();
                    ui.confirm_plan(plan);
                }
                Ok(Err(error)) => {
                    let _ = sender.send(Event::Failure(error.to_string())).await;
                }
                Err(error) => {
                    ui.finish();
                    ui.error(&error.to_string());
                }
            }
        });
    }
    fn confirm_plan(self: &Rc<Self>, plan: Plan) {
        let text = format!(
            "{}\n\n{}",
            plan.operations.join("\n\n"),
            plan.warnings.join("\n\n")
        );
        let dialog = adw::MessageDialog::builder().transient_for(&self.window).heading(if plan.options.update { format!("Update {}?", plan.options.name) } else { format!("Install {}?", plan.options.name) }).body(format!("Install for your account in:\n{}\n\n{}\n\nNo application code or installation scripts will run. Only install software you trust.", plan.destination.display(), if plan.launcher.is_some() { "The app will be available in your application menu." } else { "No application-menu launcher will be created." })).build();
        let view = gtk::TextView::builder()
            .editable(false)
            .monospace(true)
            .wrap_mode(gtk::WrapMode::WordChar)
            .height_request(260)
            .build();
        view.buffer().set_text(&text);
        let scroller = scroll(&view);
        scroller.set_max_content_height(420);
        let details = gtk::Expander::builder()
            .label("Show All Changes & Warnings")
            .child(&scroller)
            .build();
        dialog.set_extra_child(Some(&details));
        dialog.add_responses(&[
            ("cancel", "Cancel"),
            (
                "install",
                if plan.options.update {
                    "Update"
                } else {
                    "Install"
                },
            ),
        ]);
        dialog.set_response_appearance("install", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("cancel");
        let ui = self.clone();
        dialog.connect_response(None, move |_, response| {
            if response == "install" {
                ui.install_plan(plan.clone());
            }
        });
        dialog.present();
    }
    fn install_plan(self: &Rc<Self>, plan: Plan) {
        let Some(cancel) = self.begin("Installing application…") else {
            return;
        };
        let paths = self.paths.clone();
        let limits = self.state.borrow().settings.limits.clone();
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            match installer::execute(
                &paths,
                &plan,
                &limits,
                &cancel,
                progress_sender(sender.clone()),
            ) {
                Ok(manifest) => {
                    let _ = sender.send_blocking(Event::Installed(manifest));
                }
                Err(e) => {
                    let _ = sender.send_blocking(Event::Failure(e.to_string()));
                }
            }
        });
    }
    pub(super) fn extract_dialog(self: &Rc<Self>) {
        if self.state.borrow().inspection.is_none() {
            self.notify("Open an archive first");
            return;
        }
        let dialog = gtk::FileDialog::builder()
            .title("Choose Parent Folder for Extraction")
            .build();
        let ui = self.clone();
        glib::spawn_future_local(async move {
            match dialog.select_folder_future(Some(&ui.window)).await {
                Ok(folder) => {
                    if let Some(parent) = folder.path() {
                        let state = ui.state.borrow();
                        let Some(inspection) = state.inspection.clone() else {
                            return;
                        };
                        let limits = state.settings.limits.clone();
                        drop(state);
                        let name = inspection
                            .source
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();
                        let name = [
                            ".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst", ".tgz", ".tar", ".zip",
                            ".7z",
                        ]
                        .iter()
                        .find_map(|s| name.strip_suffix(s))
                        .unwrap_or(&name);
                        let destination = parent.join(format!("{name}-extracted"));
                        let Some(cancel) = ui.begin("Extracting archive…") else {
                            return;
                        };
                        let sender = ui.sender.clone();
                        std::thread::spawn(move || {
                            match archive::extract(
                                &inspection,
                                &destination,
                                &limits,
                                &cancel,
                                progress_sender(sender.clone()),
                            ) {
                                Ok(()) => {
                                    let _ = sender.send_blocking(Event::Done(format!(
                                        "Extracted to {}",
                                        destination.display()
                                    )));
                                }
                                Err(e) => {
                                    let _ = sender.send_blocking(Event::Failure(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
                Err(e) => ui.error(&e.to_string()),
            }
        });
    }
    pub(super) fn create_dialog(self: &Rc<Self>) {
        let ui = self.clone();
        glib::spawn_future_local(async move {
            let open = gtk::FileDialog::builder()
                .title("Choose Files to Archive")
                .build();
            let files = match open.open_multiple_future(Some(&ui.window)).await {
                Ok(files) => files,
                Err(e) if e.matches(gtk::DialogError::Dismissed) => return,
                Err(e) => {
                    ui.error(&e.to_string());
                    return;
                }
            };
            let sources: Vec<_> = files
                .iter::<gio::File>()
                .flatten()
                .filter_map(|f| f.path())
                .collect();
            let save = gtk::FileDialog::builder()
                .title("Save Archive — .tar.gz, .tar.xz, .tar.zst, .zip or .7z")
                .initial_name("Archive.tar.gz")
                .build();
            match save.save_future(Some(&ui.window)).await {
                Ok(file) => {
                    if let Some(destination) = file.path() {
                        let Some(format) = Format::from_path(&destination) else {
                            ui.error("Use a supported archive filename extension, such as .tar.gz or .zip.");
                            return;
                        };
                        let Some(cancel) = ui.begin("Creating archive…") else {
                            return;
                        };
                        let sender = ui.sender.clone();
                        std::thread::spawn(move || {
                            match archive::create(
                                &sources,
                                &destination,
                                format,
                                &cancel,
                                progress_sender(sender.clone()),
                            ) {
                                Ok(()) => {
                                    let _ = sender.send_blocking(Event::Done(format!(
                                        "Created {}",
                                        destination.display()
                                    )));
                                }
                                Err(e) => {
                                    let _ = sender.send_blocking(Event::Failure(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Err(e) if e.matches(gtk::DialogError::Dismissed) => {}
                Err(e) => ui.error(&e.to_string()),
            }
        });
    }
}
fn paths_contains(candidates: &[detection::Candidate], path: &std::path::Path) -> bool {
    candidates.iter().any(|c| c.path == path)
}

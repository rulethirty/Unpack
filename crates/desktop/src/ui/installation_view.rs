//! Compact installer: fixed primary action, progressively disclosed controls,
//! and a separate Files tab rather than width-dependent navigation buttons.
use super::archive_view::ArchiveView;
use super::*;

fn section(title: &str, child: &impl IsA<gtk::Widget>) -> gtk::Expander {
    let expander = gtk::Expander::builder().label(title).child(child).build();
    let icon = match title {
        "Name & Icon" => "applications-graphics-symbolic",
        "Install Location" => "folder-symbolic",
        "Launch Settings" => "media-playback-start-symbolic",
        "Advanced & Updates" => "preferences-system-symbolic",
        _ => "dialog-information-symbolic",
    };
    super::styles::disclosure(&expander, icon);
    margins(&expander, 2);
    expander
}

fn compact_dropdown(dropdown: &gtk::DropDown) {
    dropdown.add_css_class("archive-choice");
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            let label = gtk::Label::builder()
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::Middle)
                .max_width_chars(30)
                .build();
            item.set_child(Some(&label));
        }
    });
    factory.connect_bind(|_, item| {
        if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
            if let (Some(value), Some(label)) = (
                item.item().and_downcast::<gtk::StringObject>(),
                item.child().and_downcast::<gtk::Label>(),
            ) {
                label.set_text(&value.string());
                label.set_tooltip_text(Some(&value.string()));
            }
        }
    });
    dropdown.set_factory(Some(&factory));
}

impl ArchiveView {
    pub fn new() -> Self {
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        margins(&heading, 16);
        let heading_text = gtk::Box::new(gtk::Orientation::Vertical, 3);
        heading_text.set_hexpand(true);
        let title = label("Open an archive to begin", "heading");
        title.set_wrap(false);
        title.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let subtitle = label(
            "Browse files or install an application. You decide.",
            "dim-label",
        );
        heading_text.append(&title);
        heading_text.append(&subtitle);
        heading.append(&heading_text);
        let next = gtk::Button::with_label("Next Archive");
        next.set_visible(false);
        next.set_tooltip_text(Some(
            "Skip to the next archive you opened. Nothing is installed automatically.",
        ));
        heading.append(&next);
        widget.append(&heading);
        let pages = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .hhomogeneous(false)
            .vhomogeneous(false)
            .hexpand(true)
            .vexpand(true)
            .build();
        let tabs = gtk::StackSwitcher::builder()
            .stack(&pages)
            .halign(gtk::Align::Center)
            .margin_bottom(12)
            .build();
        widget.append(&tabs);
        widget.append(&pages);

        let install = gtk::Box::new(gtk::Orientation::Vertical, 0);
        // This summary never scrolls away with the advanced options.
        let hero = gtk::Box::new(gtk::Orientation::Horizontal, 20);
        margins(&hero, 24);
        let app_icon = gtk::Image::from_icon_name("application-x-executable");
        app_icon.set_pixel_size(80);
        hero.append(&app_icon);
        let hero_text = gtk::Box::new(gtk::Orientation::Vertical, 5);
        hero_text.set_hexpand(true);
        let app_name = label("Your next application", "title-1");
        let summary = label(
            "Open an archive to see its application details.",
            "dim-label",
        );
        hero_text.append(&app_name);
        hero_text.append(&summary);
        hero.append(&hero_text);
        install.append(&hero);
        let action_bar = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margins(&action_bar, 16);
        action_bar.set_margin_top(0);
        let review = gtk::Button::with_label("Install…");
        review.add_css_class("suggested-action");
        review.add_css_class("pill");
        review.set_height_request(52);
        review.set_action_name(Some("win.primary"));
        review.set_sensitive(false);
        action_bar.append(&review);
        let install_hint = label(
            "You'll review every change before installing. No administrator access needed.",
            "dim-label",
        );
        install_hint.set_justify(gtk::Justification::Center);
        install_hint.set_xalign(0.5);
        action_bar.append(&install_hint);
        install.append(&action_bar);
        let details = gtk::Box::new(gtk::Orientation::Vertical, 10);
        margins(&details, 16);
        details.set_margin_top(0);
        let attention = label("", "warning");
        attention.set_visible(false);
        details.append(&attention);
        let readme = gtk::Button::with_label("Read Installation Instructions");
        readme.add_css_class("flat");
        readme.set_visible(false);
        details.append(&readme);

        let identity = gtk::Box::new(gtk::Orientation::Vertical, 10);
        margins(&identity, 14);
        identity.append(&label("Use the name and icon found inside this archive, or make your own selection. Nothing is fetched from the internet.", "dim-label"));
        let fetch = gtk::Button::with_label("Detect Name & Icon from Archive");
        identity.append(&fetch);
        let name = gtk::Entry::builder()
            .placeholder_text("Application name")
            .build();
        identity.append(&label("Application name", "heading"));
        identity.append(&name);
        let metadata_source = label("", "dim-label");
        metadata_source.set_selectable(true);
        identity.append(&metadata_source);
        let icon_choice = gtk::DropDown::from_strings(&["Default application icon"]);
        compact_dropdown(&icon_choice);
        icon_choice.set_enable_search(true);
        identity.append(&label("Application icon", "heading"));
        identity.append(&icon_choice);
        let icon = gtk::CheckButton::with_label("Use this icon for the installed application");
        icon.set_active(true);
        identity.append(&icon);
        identity.append(&label("PNG and XPM icons are installed from the archive. SVG files remain available in Files for inspection; they are not imported because they may reference external resources.", "dim-label"));
        let identity_section = section("Name & Icon", &identity);
        details.append(&identity_section);

        let location = gtk::Box::new(gtk::Orientation::Vertical, 10);
        margins(&location, 14);
        let destination = gtk::Entry::builder()
            .placeholder_text("Installation parent directory")
            .build();
        location.append(&destination);
        location.append(&label("Recommended: ~/.local/opt — only for your account, without administrator access. ~/Applications and custom folders inside your home also work. /opt and /usr/local need a separate administrator-managed installation.", "dim-label"));
        let location_section = section("Install Location", &location);
        details.append(&location_section);

        let launch_settings = gtk::Box::new(gtk::Orientation::Vertical, 10);
        margins(&launch_settings, 14);
        launch_settings.append(&label("Main application file", "heading"));
        let executable = gtk::DropDown::from_strings(&["Inspect an archive first"]);
        compact_dropdown(&executable);
        executable.set_enable_search(true);
        launch_settings.append(&executable);
        launch_settings.append(&label("All application files, libraries and resources stay together. This selection identifies the file launched from your application menu.", "dim-label"));
        let launcher = gtk::CheckButton::with_label("Show in the application menu");
        launcher.set_active(true);
        launch_settings.append(&launcher);
        let terminal = gtk::CheckButton::with_label("Run in a terminal");
        launch_settings.append(&terminal);
        let launch_section = section("Launch Settings", &launch_settings);
        details.append(&launch_section);

        let advanced = gtk::Box::new(gtk::Orientation::Vertical, 10);
        margins(&advanced, 14);
        advanced.append(&label("Application identifier", "heading"));
        let id = gtk::Entry::builder()
            .placeholder_text("application-id")
            .build();
        advanced.append(&id);
        let update = gtk::CheckButton::with_label("Update an existing installation");
        advanced.append(&update);
        advanced.append(&label("For updates, use the existing application ID and location. A recoverable backup is kept. Modified or user-added files are never overwritten.", "dim-label"));
        let advanced_section = section("Advanced & Updates", &advanced);
        details.append(&advanced_section);

        let inspection_box = gtk::Box::new(gtk::Orientation::Vertical, 10);
        margins(&inspection_box, 14);
        let findings = label("Inspection results will appear here.", "body");
        findings.set_selectable(true);
        inspection_box.append(&findings);
        let archive_info = label("", "dim-label");
        archive_info.set_selectable(true);
        inspection_box.append(&archive_info);
        let inspection_section = section("Archive Details & Compatibility", &inspection_box);
        details.append(&inspection_section);
        details.append(&label("Only install software you trust. Archive inspection is not malware detection. Nothing inside the archive runs during installation.", "dim-label"));
        install.append(&scroll(&details));
        let clamp = adw::Clamp::builder()
            .maximum_size(680)
            .tightening_threshold(560)
            .child(&install)
            .build();
        pages.add_titled(&clamp, Some("install"), "Install");

        let browser = gtk::Box::new(gtk::Orientation::Vertical, 8);
        margins(&browser, 16);
        let toolbar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let back = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .tooltip_text("Parent archive directory")
            .build();
        back.set_sensitive(false);
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search archive contents")
            .hexpand(true)
            .build();
        toolbar.append(&back);
        toolbar.append(&search);
        let extract = gtk::Button::with_label("Extract…");
        extract.set_action_name(Some("win.extract"));
        toolbar.append(&extract);
        browser.append(&toolbar);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("boxed-list");
        list.set_placeholder(Some(
            &adw::StatusPage::builder()
                .icon_name("folder-symbolic")
                .title("Nothing to show")
                .description("Open an archive, or try another search.")
                .build(),
        ));
        browser.append(&scroll(&list));
        pages.add_titled(&browser, Some("files"), "Files");
        Self {
            widget,
            search,
            list,
            title,
            subtitle,
            findings,
            name,
            id,
            destination,
            executable,
            launcher,
            icon,
            terminal,
            update,
            review,
            pages,
            next,
            back,
            app_icon,
            app_name,
            summary,
            install_hint,
            attention,
            readme,
            fetch,
            metadata_source,
            icon_choice,
            archive_info,
            identity_section,
            location_section,
            launch_section,
            advanced_section,
            inspection_section,
            icon_paths: RefCell::new(vec![]),
            directory: RefCell::new(PathBuf::new()),
            entry_paths: RefCell::new(vec![]),
            visible_entries: RefCell::new(vec![]),
        }
    }
}

impl Window {
    pub(super) fn current_installed_app(&self) -> Option<Manifest> {
        let state = self.state.borrow();
        let inspection = state.inspection.as_ref()?;
        let id = self.archive.id.text();
        state
            .applications
            .iter()
            .find(|app| app.id == id.as_str() && app.source_sha256 == inspection.sha256)
            .cloned()
    }

    pub(super) fn sync_install_action(&self) {
        let installed = self.current_installed_app();
        let updating = self.archive.update.is_active();
        self.archive
            .review
            .set_label(if installed.is_some() && !updating {
                "Open"
            } else if updating {
                "Update…"
            } else {
                "Install…"
            });
        let state = self.state.borrow();
        self.archive
            .review
            .set_sensitive(!state.busy && state.inspection.is_some());
        if let Some(app) = installed.filter(|_| !updating) {
            self.archive.summary.set_text(&format!(
                "Installed for your account\n{}",
                app.destination.display()
            ));
            self.archive.install_hint.set_text(
                "Ready to use. Open the app here or find it in your system's application menu.",
            );
            self.archive.attention.set_visible(false);
        } else {
            self.archive
                .install_hint
                .set_text(if unpack_core::config::sandboxed_package() {
                    "Flatpak cannot install host applications. Use the AppImage or native edition."
                } else {
                    "Review every change before installing. No administrator access needed."
                });
            if let (Some(inspection), Some(detection)) = (&state.inspection, &state.detection) {
                self.archive.summary.set_text(&format!(
                    "{} · {}\nFor your account. All application files stay together.",
                    detection.kind.label(),
                    super::archive_view::size(inspection.total_size)
                ));
            }
        }
    }

    pub(super) fn primary_action(self: &Rc<Self>) {
        if self.state.borrow().busy {
            return;
        }
        if !self.archive.update.is_active() {
            if let Some(app) = self.current_installed_app() {
                self.launch(&app);
                return;
            }
        }
        self.review_install();
    }

    pub(super) fn selected_archive_icon(&self) -> Option<PathBuf> {
        self.archive
            .icon_choice
            .selected()
            .checked_sub(1)
            .and_then(|i| self.archive.icon_paths.borrow().get(i as usize).cloned())
    }

    pub(super) fn update_queue_ui(&self) {
        let count = self.state.borrow().queue.len();
        self.archive.next.set_visible(count > 0);
        self.archive
            .next
            .set_label(&format!("Next Archive ({count})"));
        self.archive.next.set_sensitive(!self.state.borrow().busy);
    }

    pub(super) fn refresh_archive_icon(&self) {
        self.archive
            .app_icon
            .set_icon_name(Some("application-x-executable"));
        if !self.archive.icon.is_active() {
            return;
        }
        let Some(path) = self.selected_archive_icon() else {
            return;
        };
        let Some(inspection) = self.state.borrow().inspection.clone() else {
            return;
        };
        let sender = self.sender.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<super::icon_decode::Pixels, String> {
                let bytes = archive::read_member(
                    &inspection,
                    &path,
                    unpack_core::identity::MAX_ICON_BYTES,
                    &Cancellation::default(),
                )
                .map_err(|e| e.to_string())?;
                super::icon_decode::decode(&path, bytes)
            })();
            let _ = sender.send_blocking(Event::ArchiveIcon {
                archive_hash: inspection.sha256,
                path,
                png: result,
            });
        });
    }
}

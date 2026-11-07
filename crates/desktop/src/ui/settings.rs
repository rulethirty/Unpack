use super::*;
use unpack_core::config::OpenBehavior;

impl Window {
    pub(super) fn preferences(self: &Rc<Self>) {
        let window = adw::PreferencesWindow::builder()
            .transient_for(&self.window)
            .title("Preferences")
            .default_width(650)
            .default_height(680)
            .build();
        let page = adw::PreferencesPage::builder()
            .title("General")
            .icon_name("preferences-system-symbolic")
            .build();
        let general = adw::PreferencesGroup::builder().title("Opening Archives").description("Choose what happens when Unpack receives an archive. This does not change your file manager’s default application.").build();
        let behavior = adw::ComboRow::builder()
            .title("Default behavior")
            .model(&gtk::StringList::new(&[
                "Ask every time",
                "Browse in Unpack",
                "Inspect as an application",
                "Use another archive manager",
            ]))
            .build();
        behavior.set_selected(match self.state.borrow().settings.open_behavior {
            OpenBehavior::Ask => 0,
            OpenBehavior::Browser => 1,
            OpenBehavior::Installer => 2,
            OpenBehavior::External => 3,
        });
        general.add(&behavior);
        let appearance = adw::PreferencesGroup::builder().title("Appearance").build();
        let theme = adw::ComboRow::builder()
            .title("Color scheme")
            .model(&gtk::StringList::new(&["Follow system", "Light", "Dark"]))
            .build();
        theme.set_selected(match self.state.borrow().settings.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        });
        appearance.add(&theme);
        let hidden = adw::SwitchRow::builder()
            .title("Show hidden archive files")
            .active(self.state.borrow().settings.show_hidden)
            .build();
        appearance.add(&hidden);
        let restore = adw::SwitchRow::builder()
            .title("Restore window size and maximized state")
            .subtitle("Window placement is controlled by the compositor on Wayland")
            .active(self.state.borrow().settings.restore_window)
            .build();
        appearance.add(&restore);
        let install = adw::PreferencesGroup::builder().title("Installation").description("User-level installations only. Every installation requires an explicit plan confirmation.").build();
        let destination = adw::EntryRow::builder()
            .title("Default installation parent directory")
            .text(
                self.state
                    .borrow()
                    .settings
                    .installation_directory
                    .as_ref()
                    .unwrap_or(&self.paths.default_install)
                    .to_string_lossy(),
            )
            .build();
        install.add(&destination);
        let launcher = adw::SwitchRow::builder()
            .title("Create application-menu launchers")
            .active(self.state.borrow().settings.create_launcher)
            .build();
        install.add(&launcher);
        let icons = adw::SwitchRow::builder()
            .title("Install detected raster icons")
            .subtitle("SVG files can be browsed but are not imported as launcher icons")
            .active(self.state.borrow().settings.install_icons)
            .build();
        install.add(&icons);
        let security = adw::PreferencesGroup::builder().title("Security").description("Path validation, no-follow extraction and script protections are always enabled. Inspection is not malware detection.").build();
        let limits = self.state.borrow().settings.limits.clone();
        let max_size = adw::SpinRow::with_range(1.0, 1024.0, 1.0);
        max_size.set_title("Maximum extracted size (GiB)");
        max_size.set_value((limits.max_total_bytes >> 30).max(1) as f64);
        security.add(&max_size);
        let max_entries = adw::SpinRow::with_range(1.0, 1_000_000.0, 1000.0);
        max_entries.set_title("Maximum archive entries");
        max_entries.set_value(limits.max_entries as f64);
        security.add(&max_entries);
        let max_ratio = adw::SpinRow::with_range(1.0, 10000.0, 10.0);
        max_ratio.set_title("Maximum compression ratio");
        max_ratio.set_value(limits.max_ratio as f64);
        security.add(&max_ratio);
        let integrations = adw::PreferencesGroup::builder().title("Desktop Integration").description("Installing Unpack does not claim archive associations. Your desktop’s Open With dialog can select Unpack per user. The packaged launcher offers Browse Archive and Install Application actions where supported by the file manager.").build();
        let info = adw::ActionRow::builder()
            .title("Default archive associations are never changed automatically")
            .subtitle(
                "Use your file manager’s Open With preferences to choose a handler explicitly",
            )
            .build();
        integrations.add(&info);
        let save = gtk::Button::with_label("Save Preferences");
        save.add_css_class("suggested-action");
        let save_group = adw::PreferencesGroup::new();
        save_group.add(&save);
        for group in [
            &general,
            &appearance,
            &install,
            &security,
            &integrations,
            &save_group,
        ] {
            page.add(group);
        }
        window.add(&page);
        let weak = Rc::downgrade(self);
        let pref_window = window.clone();
        save.connect_clicked(move |_| { if let Some(ui) = weak.upgrade() {
            let path = PathBuf::from(destination.text().as_str()); if !path.is_absolute() || !path.starts_with(&ui.paths.home) || path == ui.paths.home { ui.error("Choose an absolute installation directory inside your home, such as ~/.local/opt (with the full home path)."); return; }
            let mut settings = ui.state.borrow().settings.clone(); settings.open_behavior = match behavior.selected() { 1 => OpenBehavior::Browser, 2 => OpenBehavior::Installer, 3 => OpenBehavior::External, _ => OpenBehavior::Ask };
            settings.theme = match theme.selected() { 1 => Theme::Light, 2 => Theme::Dark, _ => Theme::System }; settings.show_hidden = hidden.is_active(); settings.restore_window = restore.is_active(); settings.installation_directory = Some(path); settings.create_launcher = launcher.is_active(); settings.install_icons = icons.is_active(); settings.limits.max_total_bytes = (max_size.value() as u64) << 30; settings.limits.max_entries = max_entries.value() as usize; settings.limits.max_ratio = max_ratio.value() as u64;
            match settings.save(&ui.paths) { Ok(()) => { apply_theme(settings.theme); ui.state.borrow_mut().settings = settings; ui.refresh_entries(); ui.notify("Preferences saved"); pref_window.close(); }, Err(e) => ui.error(&e.to_string()) }
        } });
        window.present();
    }
    pub(super) fn onboarding(self: &Rc<Self>) {
        let dialog = adw::MessageDialog::builder().transient_for(&self.window).heading("Welcome to Unpack")
            .body("Make sense of downloaded archives.\n\n1. Inspect files and read instructions without running anything.\n2. Review a portable application’s installation plan.\n3. Install under ~/.local/opt without administrator access.\n4. Find it in your application menu and manage it here.\n5. Remove only the files Unpack recorded.\n\nScripts never run automatically. Your archive associations stay unchanged.").build();
        dialog.add_responses(&[("skip", "Skip"), ("start", "Get Started")]);
        dialog.set_response_appearance("start", adw::ResponseAppearance::Suggested);
        dialog.set_close_response("skip");
        let weak = Rc::downgrade(self);
        dialog.connect_response(None, move |_, _| {
            if let Some(ui) = weak.upgrade() {
                ui.state.borrow_mut().settings.onboarding_complete = true;
                if ui.state.borrow().settings_writable {
                    if let Err(e) = ui.state.borrow().settings.save(&ui.paths) {
                        ui.error(&e.to_string());
                    }
                }
            }
        });
        dialog.present();
    }
}

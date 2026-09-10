use anyhow::Result;
use gtk::{gdk, gio, glib, prelude::*};
use reelmux::{
    language,
    media::{self, AudioMode, Document, ExportEvent, Track},
    metadata::{
        Artwork, ArtworkCandidate, Client as MetadataClient, MediaKind, MetadataResult, Provider,
        SearchHit, SearchQuery,
    },
};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

struct Ui {
    window: gtk::ApplicationWindow,
    document: RefCell<Option<Document>>,
    dirty: Cell<bool>,
    loading: Cell<bool>,
    busy: Cell<bool>,
    closing: Cell<bool>,
    cancel: RefCell<Option<Arc<AtomicBool>>>,
    model: gio::ListStore,
    stack: gtk::Stack,
    editor: gtk::Box,
    open: gtk::Button,
    save: gtk::Button,
    cancel_button: gtk::Button,
    filename: gtk::Label,
    summary: gtk::Label,
    status: gtk::Label,
    progress: gtk::ProgressBar,
    fields: Vec<(&'static str, gtk::Entry)>,
    description: gtk::TextView,
    chapters: gtk::Label,
    artwork: gtk::Picture,
    artwork_caption: gtk::Label,
    imported_details: gtk::Label,
    metadata_browser: RefCell<Option<Rc<MetadataBrowser>>>,
}

fn label(text: &str, class: &str) -> gtk::Label {
    let w = gtk::Label::new(Some(text));
    w.set_xalign(0.0);
    w.add_css_class(class);
    w
}
fn margins(w: &impl IsA<gtk::Widget>, n: i32) {
    w.set_margin_top(n);
    w.set_margin_bottom(n);
    w.set_margin_start(n);
    w.set_margin_end(n);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportMode {
    MetadataAndArtwork,
    ArtworkOnly,
}

struct MetadataBrowser {
    window: gtk::Window,
    owner: std::rc::Weak<Ui>,
    provider: gtk::DropDown,
    kind: gtk::DropDown,
    term: gtk::Entry,
    language: gtk::Entry,
    country: gtk::Entry,
    season: gtk::Entry,
    episode: gtk::Entry,
    search: gtk::Button,
    import: gtk::Button,
    results: gtk::ListBox,
    status: gtk::Label,
    spinner: gtk::Spinner,
    mode: ImportMode,
    hits: RefCell<Vec<SearchHit>>,
    artwork_choices: RefCell<Vec<ArtworkCandidate>>,
    resolved: RefCell<Option<MetadataResult>>,
    query: RefCell<Option<SearchQuery>>,
    selected: Cell<Option<usize>>,
    busy: Cell<bool>,
}

impl MetadataBrowser {
    fn new(owner: &Rc<Ui>, mode: ImportMode) -> Rc<Self> {
        let artwork_only = mode == ImportMode::ArtworkOnly;
        let window = gtk::Window::builder()
            .title(if artwork_only {
                "Cambia locandina"
            } else {
                "Importa metadati"
            })
            .transient_for(&owner.window)
            .modal(true)
            .default_width(760)
            .default_height(650)
            .build();
        let root = gtk::Box::new(gtk::Orientation::Vertical, 14);
        margins(&root, 20);
        root.append(&label(
            if artwork_only {
                "Cerca una nuova locandina senza modificare i metadati"
            } else {
                "Cerca film e serie nei provider online"
            },
            "section-title",
        ));
        let form = gtk::Grid::builder()
            .column_spacing(10)
            .row_spacing(8)
            .build();
        let provider = gtk::DropDown::from_strings(
            &Provider::ALL
                .iter()
                .map(|provider| provider.label())
                .collect::<Vec<_>>(),
        );
        let kind = gtk::DropDown::from_strings(&["Film", "Serie TV"]);
        let term = gtk::Entry::new();
        term.set_hexpand(true);
        let language = gtk::Entry::new();
        language.set_text("it-IT");
        language.set_width_chars(8);
        let country = gtk::Entry::new();
        country.set_text("IT");
        country.set_width_chars(4);
        let season = gtk::Entry::new();
        season.set_width_chars(4);
        season.set_input_purpose(gtk::InputPurpose::Digits);
        let episode = gtk::Entry::new();
        episode.set_width_chars(4);
        episode.set_input_purpose(gtk::InputPurpose::Digits);
        if let Some(doc) = owner.document.borrow().as_ref() {
            let title = doc
                .metadata
                .get("show")
                .or_else(|| doc.metadata.get("title"))
                .cloned()
                .unwrap_or_else(|| {
                    doc.path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
            term.set_text(&title);
            if doc
                .metadata
                .get("show")
                .is_some_and(|value| !value.is_empty())
            {
                kind.set_selected(1);
            }
            season.set_text(
                doc.metadata
                    .get("season_number")
                    .map(String::as_str)
                    .unwrap_or(""),
            );
            episode.set_text(
                doc.metadata
                    .get("episode_sort")
                    .map(String::as_str)
                    .unwrap_or(""),
            );
        }
        for (row, title, widget) in [
            (0, "Provider", provider.clone().upcast::<gtk::Widget>()),
            (1, "Tipo", kind.clone().upcast()),
            (2, "Titolo", term.clone().upcast()),
            (3, "Lingua", language.clone().upcast()),
            (4, "Paese", country.clone().upcast()),
            (5, "Stagione", season.clone().upcast()),
            (6, "Episodio", episode.clone().upcast()),
        ] {
            form.attach(&label(title, "field-label"), 0, row, 1, 1);
            form.attach(&widget, 1, row, 1, 1);
        }
        root.append(&form);
        let credentials = label(
            "TMDB: TMDB_API_TOKEN o TMDB_API_KEY. TVDB: TVDB_API_KEY e, se richiesto, TVDB_PIN.",
            "muted",
        );
        credentials.set_wrap(true);
        root.append(&credentials);
        let credits_row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let tmdb_icon = gio::BytesIcon::new(&glib::Bytes::from_static(include_bytes!(
            "../data/tmdb-logo.svg"
        )));
        let tmdb_logo = gtk::Image::from_gicon(&tmdb_icon);
        tmdb_logo.set_pixel_size(92);
        tmdb_logo.set_tooltip_text(Some("Logo ufficiale TMDB"));
        credits_row.append(&tmdb_logo);
        let credits = gtk::Label::new(None);
        credits.set_xalign(0.0);
        credits.set_wrap(true);
        credits.set_hexpand(true);
        credits.set_markup(
            "<small>This product uses the TMDB API but is not endorsed or certified by TMDB. Metadati TheTVDB: <a href=\"https://thetvdb.com/\">thetvdb.com</a>.</small>",
        );
        credits_row.append(&credits);
        root.append(&credits_row);
        let action_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let search = gtk::Button::with_label("Cerca");
        search.add_css_class("suggested-action");
        let spinner = gtk::Spinner::new();
        action_row.append(&search);
        action_row.append(&spinner);
        root.append(&action_row);
        let results = gtk::ListBox::new();
        results.set_selection_mode(gtk::SelectionMode::Single);
        results.add_css_class("boxed-list");
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .min_content_height(240)
            .child(&results)
            .build();
        root.append(&scroll);
        let status = label("Imposta la ricerca e scegli un risultato.", "muted");
        status.set_wrap(true);
        root.append(&status);
        let footer = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        footer.set_halign(gtk::Align::End);
        let cancel = gtk::Button::with_label("Chiudi");
        let import = gtk::Button::with_label(if artwork_only {
            "Mostra locandine"
        } else {
            "Continua"
        });
        import.add_css_class("suggested-action");
        import.set_sensitive(false);
        footer.append(&cancel);
        footer.append(&import);
        root.append(&footer);
        window.set_child(Some(&root));
        let browser = Rc::new(Self {
            window,
            owner: Rc::downgrade(owner),
            provider,
            kind,
            term,
            language,
            country,
            season,
            episode,
            search,
            import,
            results,
            status,
            spinner,
            mode,
            hits: RefCell::new(Vec::new()),
            artwork_choices: RefCell::new(Vec::new()),
            resolved: RefCell::new(None),
            query: RefCell::new(None),
            selected: Cell::new(None),
            busy: Cell::new(false),
        });
        let weak = Rc::downgrade(&browser);
        browser.kind.connect_selected_notify(move |kind| {
            if let Some(browser) = weak.upgrade() {
                let tv = kind.selected() == 1;
                browser.season.set_sensitive(tv);
                browser.episode.set_sensitive(tv);
            }
        });
        let tv = browser.kind.selected() == 1;
        browser.season.set_sensitive(tv);
        browser.episode.set_sensitive(tv);
        let weak = Rc::downgrade(&browser);
        browser.results.connect_row_selected(move |_, row| {
            if let Some(browser) = weak.upgrade() {
                browser
                    .selected
                    .set(row.and_then(|row| usize::try_from(row.index()).ok()));
                browser
                    .import
                    .set_sensitive(!browser.busy.get() && row.is_some());
            }
        });
        let weak = Rc::downgrade(&browser);
        browser.search.connect_clicked(move |_| {
            if let Some(browser) = weak.upgrade() {
                browser.start_search();
            }
        });
        let weak = Rc::downgrade(&browser);
        browser.term.connect_activate(move |_| {
            if let Some(browser) = weak.upgrade() {
                browser.start_search();
            }
        });
        let weak = Rc::downgrade(&browser);
        browser.import.connect_clicked(move |_| {
            if let Some(browser) = weak.upgrade() {
                browser.start_import();
            }
        });
        let weak = Rc::downgrade(&browser);
        cancel.connect_clicked(move |_| {
            if let Some(browser) = weak.upgrade()
                && !browser.busy.get()
            {
                browser.window.close();
            }
        });
        browser.window.present();
        browser
    }

    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.window.set_deletable(!busy);
        self.search.set_sensitive(!busy);
        self.import
            .set_sensitive(!busy && self.selected.get().is_some());
        if busy {
            self.spinner.start();
        } else {
            self.spinner.stop();
        }
        if let Some(owner) = self.owner.upgrade() {
            owner.set_busy(busy);
        }
    }

    fn initial_action_label(&self) -> &'static str {
        if self.mode == ImportMode::ArtworkOnly {
            "Mostra locandine"
        } else {
            "Continua"
        }
    }

    fn artwork_action_label(&self) -> &'static str {
        if self.mode == ImportMode::ArtworkOnly {
            "Imposta locandina"
        } else {
            "Importa metadati e locandina"
        }
    }

    fn optional_number(entry: &gtk::Entry, name: &str) -> Result<Option<u32>> {
        let text = entry.text();
        let text = text.trim();
        if text.is_empty() {
            Ok(None)
        } else {
            text.parse()
                .map(Some)
                .map_err(|_| anyhow::anyhow!("{name} deve essere un numero intero"))
        }
    }

    fn current_query(&self) -> Result<SearchQuery> {
        let provider = Provider::ALL
            .get(self.provider.selected() as usize)
            .copied()
            .unwrap_or(Provider::AppleTv);
        Ok(SearchQuery {
            provider,
            kind: if self.kind.selected() == 1 {
                MediaKind::TvShow
            } else {
                MediaKind::Movie
            },
            term: self.term.text().into(),
            language: self.language.text().into(),
            country: self.country.text().into(),
            season: Self::optional_number(&self.season, "Stagione")?,
            episode: Self::optional_number(&self.episode, "Episodio")?,
        })
    }

    fn start_search(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        let query = match self.current_query() {
            Ok(query) => query,
            Err(error) => {
                if let Some(owner) = self.owner.upgrade() {
                    owner.error("Ricerca non valida", error);
                }
                return;
            }
        };
        self.set_busy(true);
        self.status
            .set_text(&format!("Ricerca su {}…", query.provider));
        *self.resolved.borrow_mut() = None;
        self.artwork_choices.borrow_mut().clear();
        self.import.set_label(self.initial_action_label());
        self.selected.set(None);
        self.import.set_sensitive(false);
        let (tx, rx) = mpsc::channel();
        let worker_query = query.clone();
        thread::spawn(move || {
            let _ = tx.send(MetadataClient::default().search(&worker_query));
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(browser) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(result) => {
                    browser.set_busy(false);
                    match result {
                        Ok(hits) => browser.display_hits(query.clone(), hits),
                        Err(error) => {
                            browser.status.set_text("Ricerca non riuscita.");
                            if let Some(owner) = browser.owner.upgrade() {
                                owner.error(
                                    "Ricerca dei metadati non riuscita",
                                    format!("{error:#}"),
                                );
                            }
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    browser.set_busy(false);
                    browser.status.set_text("Ricerca interrotta.");
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    }

    fn display_hits(&self, query: SearchQuery, hits: Vec<SearchHit>) {
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        *self.query.borrow_mut() = Some(query);
        *self.resolved.borrow_mut() = None;
        self.artwork_choices.borrow_mut().clear();
        self.import.set_label(self.initial_action_label());
        *self.hits.borrow_mut() = hits;
        for hit in self.hits.borrow().iter() {
            let row = gtk::Box::new(gtk::Orientation::Vertical, 4);
            margins(&row, 10);
            let title = label(&hit.title, "heading");
            title.set_wrap(true);
            row.append(&title);
            if !hit.subtitle.is_empty() {
                row.append(&label(&hit.subtitle, "muted"));
            }
            if !hit.overview.is_empty() {
                let overview = label(&hit.overview, "muted");
                overview.set_wrap(true);
                overview.set_lines(2);
                overview.set_ellipsize(gtk::pango::EllipsizeMode::End);
                row.append(&overview);
            }
            self.results.append(&row);
        }
        let count = self.hits.borrow().len();
        self.status.set_text(if count == 0 {
            "Nessun risultato. Prova un altro titolo, paese o provider."
        } else {
            "Seleziona un risultato da importare."
        });
        if count > 0
            && let Some(row) = self.results.first_child().and_downcast::<gtk::ListBoxRow>()
        {
            self.results.select_row(Some(&row));
        }
    }

    fn start_import(self: &Rc<Self>) {
        if self.busy.get() {
            return;
        }
        if self.resolved.borrow().is_some() {
            self.start_artwork_import();
            return;
        }
        let Some(query) = self.query.borrow().clone() else {
            return;
        };
        let Some(hit) = self
            .selected
            .get()
            .and_then(|index| self.hits.borrow().get(index).cloned())
        else {
            return;
        };
        self.set_busy(true);
        self.status
            .set_text("Caricamento dei dettagli e delle locandine…");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let client = MetadataClient::default();
            let result = client.resolve(&hit, &query).map(|metadata| {
                let jobs: Vec<_> = metadata
                    .artwork_candidates()
                    .into_iter()
                    .take(16)
                    .map(|candidate| {
                        thread::spawn(move || {
                            let preview = MetadataClient::default()
                                .download_artwork_preview(&candidate)
                                .ok();
                            (candidate, preview)
                        })
                    })
                    .collect();
                let choices: Vec<_> = jobs.into_iter().filter_map(|job| job.join().ok()).collect();
                (metadata, choices)
            });
            let _ = tx.send(result);
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(browser) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(result) => {
                    browser.set_busy(false);
                    match result {
                        Ok((metadata, choices)) => {
                            if choices.is_empty() {
                                if browser.mode == ImportMode::MetadataAndArtwork {
                                    if let Some(owner) = browser.owner.upgrade() {
                                        let provider = metadata.provider;
                                        owner.apply_imported_metadata(metadata, None);
                                        owner.status.set_text(&format!(
                                            "Metadati importati da {provider}. Nessuna locandina disponibile."
                                        ));
                                    }
                                    browser.window.close();
                                } else {
                                    browser.status.set_text(
                                        "Il risultato selezionato non contiene locandine.",
                                    );
                                }
                            } else {
                                browser.display_artworks(metadata, choices);
                            }
                        }
                        Err(error) => {
                            browser.status.set_text("Importazione non riuscita.");
                            if let Some(owner) = browser.owner.upgrade() {
                                owner.error(
                                    "Caricamento delle locandine non riuscito",
                                    format!("{error:#}"),
                                );
                            }
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    browser.set_busy(false);
                    browser.status.set_text("Importazione interrotta.");
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    }

    fn display_artworks(
        &self,
        metadata: MetadataResult,
        choices: Vec<(ArtworkCandidate, Option<Artwork>)>,
    ) {
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        self.hits.borrow_mut().clear();
        self.selected.set(None);
        *self.resolved.borrow_mut() = Some(metadata);
        *self.artwork_choices.borrow_mut() =
            choices.iter().map(|(choice, _)| choice.clone()).collect();
        for (choice, preview) in choices {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 14);
            margins(&row, 10);
            let picture = gtk::Picture::new();
            picture.set_size_request(110, 145);
            picture.set_content_fit(gtk::ContentFit::Contain);
            picture.set_can_shrink(true);
            picture.add_css_class("artwork-thumbnail");
            if let Some(preview) = preview {
                let bytes = glib::Bytes::from(&preview.bytes);
                if let Ok(texture) = gdk::Texture::from_bytes(&bytes) {
                    picture.set_paintable(Some(&texture));
                }
            }
            row.append(&picture);
            let text = gtk::Box::new(gtk::Orientation::Vertical, 5);
            text.set_valign(gtk::Align::Center);
            text.set_hexpand(true);
            text.append(&label(&choice.label, "heading"));
            text.append(&label(choice.provider.label(), "muted"));
            row.append(&text);
            self.results.append(&row);
        }
        self.import.set_label(self.artwork_action_label());
        let count = self.artwork_choices.borrow().len();
        self.status.set_text(&format!(
            "Scegli una delle {count} locandine disponibili. La prima è preselezionata."
        ));
        if let Some(row) = self.results.first_child().and_downcast::<gtk::ListBoxRow>() {
            self.results.select_row(Some(&row));
        }
    }

    fn start_artwork_import(self: &Rc<Self>) {
        let Some(metadata) = self.resolved.borrow().clone() else {
            return;
        };
        let Some(candidate) = self
            .selected
            .get()
            .and_then(|index| self.artwork_choices.borrow().get(index).cloned())
        else {
            return;
        };
        self.set_busy(true);
        self.status.set_text("Download della locandina originale…");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = MetadataClient::default()
                .download_artwork_candidate(&candidate)
                .map(|artwork| (metadata, artwork));
            let _ = tx.send(result);
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(browser) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            match rx.try_recv() {
                Ok(result) => {
                    browser.set_busy(false);
                    match result {
                        Ok((metadata, artwork)) => {
                            if let Some(owner) = browser.owner.upgrade() {
                                if browser.mode == ImportMode::ArtworkOnly {
                                    let provider = artwork.provider;
                                    owner.apply_imported_artwork(artwork);
                                    owner.status.set_text(&format!(
                                        "Locandina impostata da {provider}. Esporta per salvarla nel file."
                                    ));
                                } else {
                                    let provider = metadata.provider;
                                    owner.apply_imported_metadata(metadata, Some(artwork));
                                    owner.status.set_text(&format!(
                                        "Metadati e locandina importati da {provider}. Esporta per salvarli nel file."
                                    ));
                                }
                            }
                            browser.window.close();
                        }
                        Err(error) => {
                            browser.status.set_text("Download non riuscito.");
                            if let Some(owner) = browser.owner.upgrade() {
                                owner.error(
                                    "Download della locandina non riuscito",
                                    format!("{error:#}"),
                                );
                            }
                        }
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    browser.set_busy(false);
                    browser.status.set_text("Download interrotto.");
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    }
}

impl Ui {
    fn new(app: &gtk::Application) -> Rc<Self> {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("ReelMux")
            .default_width(1180)
            .default_height(800)
            .build();
        window.set_size_request(900, 620);
        let header = gtk::HeaderBar::new();
        let brand = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let logo = gtk::Image::from_icon_name("video-x-generic-symbolic");
        logo.add_css_class("brand-icon");
        brand.append(&logo);
        brand.append(&label("ReelMux", "heading"));
        brand.append(&label("PROTOTIPO", "badge"));
        header.set_title_widget(Some(&brand));
        let open = gtk::Button::with_label("Apri file");
        open.set_tooltip_text(Some("Apri un file (Ctrl+O)"));
        header.pack_start(&open);
        let save = gtk::Button::with_label("Esporta MP4…");
        save.add_css_class("suggested-action");
        save.set_sensitive(false);
        save.set_tooltip_text(Some("Esporta un nuovo MP4 (Ctrl+Shift+S)"));
        header.pack_end(&save);
        window.set_titlebar(Some(&header));
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        let empty = gtk::Box::new(gtk::Orientation::Vertical, 18);
        empty.set_valign(gtk::Align::Center);
        empty.set_halign(gtk::Align::Center);
        margins(&empty, 48);
        let icon = gtk::Image::from_icon_name("video-x-generic-symbolic");
        icon.set_pixel_size(76);
        icon.add_css_class("hero-icon");
        empty.append(&icon);
        for (text, class) in [
            ("Le tue tracce. Il tuo MP4.", "hero-title"),
            (
                "Organizza audio e sottotitoli, modifica i metadati\ne salva una nuova copia del tuo video.",
                "muted",
            ),
        ] {
            let w = label(text, class);
            w.set_justify(gtk::Justification::Center);
            w.set_halign(gtk::Align::Center);
            empty.append(&w);
        }
        let empty_open = gtk::Button::with_label("Scegli un file…");
        empty_open.add_css_class("suggested-action");
        empty_open.add_css_class("pill");
        empty_open.set_halign(gtk::Align::Center);
        empty.append(&empty_open);
        let hint = label(
            "MP4 · M4V · MOV · MKV\nPuoi anche trascinare qui un file",
            "muted",
        );
        hint.set_justify(gtk::Justification::Center);
        hint.set_halign(gtk::Align::Center);
        empty.append(&hint);
        stack.add_named(&empty, Some("empty"));
        let editor = gtk::Box::new(gtk::Orientation::Vertical, 20);
        margins(&editor, 24);
        let filename = label("", "document-title");
        filename.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        let summary = label("", "muted");
        let info = gtk::Box::new(gtk::Orientation::Vertical, 6);
        info.append(&filename);
        info.append(&summary);
        editor.append(&info);
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_wide_handle(true);
        paned.set_position(750);
        paned.set_vexpand(true);
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        let tracks_panel = gtk::Box::new(gtk::Orientation::Vertical, 12);
        tracks_panel.set_margin_end(16);
        let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let title = label("Tracce", "section-title");
        title.set_hexpand(true);
        bar.append(&title);
        let add = gtk::Button::with_label("+ Sottotitoli SRT");
        add.set_tooltip_text(Some("Aggiungi SRT (Ctrl+I)"));
        bar.append(&add);
        tracks_panel.append(&bar);
        let model = gio::ListStore::new::<glib::BoxedAnyObject>();
        let table = gtk::ColumnView::new(Some(gtk::MultiSelection::new(Some(model.clone()))));
        table.set_show_row_separators(true);
        table.add_css_class("tracks-table");
        let scroll = gtk::ScrolledWindow::builder()
            .vexpand(true)
            .hexpand(true)
            .min_content_height(160)
            .child(&table)
            .build();
        scroll.add_css_class("card");
        tracks_panel.append(&scroll);
        let notes = label(
            "Seleziona le tracce da includere. Il video viene copiato.\nI sottotitoli testuali vengono convertiti in testo MP4.",
            "muted",
        );
        notes.set_wrap(true);
        tracks_panel.append(&notes);
        let chapters = label("", "muted");
        chapters.set_wrap(true);
        tracks_panel.append(&chapters);
        paned.set_start_child(Some(&tracks_panel));
        let meta = gtk::Box::new(gtk::Orientation::Vertical, 14);
        meta.set_size_request(280, -1);
        meta.set_margin_start(16);
        let metadata_bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let metadata_title = label("Metadati", "section-title");
        metadata_title.set_hexpand(true);
        metadata_bar.append(&metadata_title);
        let metadata_search = gtk::Button::with_label("Cerca online…");
        metadata_search.set_tooltip_text(Some("Importa metadati e locandina (Ctrl+M)"));
        metadata_bar.append(&metadata_search);
        meta.append(&metadata_bar);
        let artwork = gtk::Picture::new();
        artwork.set_size_request(-1, 190);
        artwork.set_content_fit(gtk::ContentFit::Contain);
        artwork.set_can_shrink(true);
        artwork.add_css_class("card");
        meta.append(&artwork);
        let artwork_caption = label("Nessuna locandina importata", "muted");
        artwork_caption.set_wrap(true);
        meta.append(&artwork_caption);
        let artwork_search = gtk::Button::with_label("Cambia locandina…");
        artwork_search.set_tooltip_text(Some(
            "Cerca e imposta soltanto una nuova locandina (Ctrl+Shift+M)",
        ));
        meta.append(&artwork_search);
        let mut fields = Vec::new();
        for (key, title, placeholder) in [
            ("title", "Titolo", "Titolo del film o dell’episodio"),
            ("date", "Data di uscita", "2026 oppure 2026-09-09"),
            ("genre", "Genere", "Drammatico, documentario…"),
            ("show", "Serie TV", "Nome della serie"),
            ("season_number", "Stagione", "1"),
            ("episode_sort", "Episodio", "1"),
        ] {
            let group = gtk::Box::new(gtk::Orientation::Vertical, 5);
            group.append(&label(title, "field-label"));
            let entry = gtk::Entry::new();
            entry.set_placeholder_text(Some(placeholder));
            entry.set_tooltip_text(Some(title));
            if key == "season_number" || key == "episode_sort" {
                entry.set_input_purpose(gtk::InputPurpose::Digits);
            }
            group.append(&entry);
            meta.append(&group);
            fields.push((key, entry));
        }
        meta.append(&label("Descrizione", "field-label"));
        let description = gtk::TextView::new();
        description.set_wrap_mode(gtk::WrapMode::WordChar);
        description.set_top_margin(8);
        description.set_bottom_margin(8);
        description.set_left_margin(10);
        description.set_right_margin(10);
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(100)
            .child(&description)
            .build();
        scroll.add_css_class("card");
        meta.append(&scroll);
        let imported_details = label("", "muted");
        imported_details.set_wrap(true);
        imported_details.set_selectable(true);
        meta.append(&imported_details);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&meta)
            .build();
        paned.set_end_child(Some(&scroll));
        editor.append(&paned);
        stack.add_named(&editor, Some("editor"));
        root.append(&stack);
        let footer = gtk::Box::new(gtk::Orientation::Vertical, 8);
        footer.add_css_class("footer");
        margins(&footer, 16);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        let status = label("Pronto. Il file originale resta invariato.", "muted");
        status.set_hexpand(true);
        status.set_wrap(true);
        let cancel_button = gtk::Button::with_label("Annulla");
        cancel_button.set_visible(false);
        row.append(&status);
        row.append(&cancel_button);
        footer.append(&row);
        let progress = gtk::ProgressBar::new();
        progress.set_visible(false);
        footer.append(&progress);
        root.append(&footer);
        window.set_child(Some(&root));
        let ui = Rc::new(Self {
            window,
            document: RefCell::new(None),
            dirty: Cell::new(false),
            loading: Cell::new(false),
            busy: Cell::new(false),
            closing: Cell::new(false),
            cancel: RefCell::new(None),
            model,
            stack,
            editor,
            open,
            save,
            cancel_button,
            filename,
            summary,
            status,
            progress,
            fields,
            description,
            chapters,
            artwork,
            artwork_caption,
            imported_details,
            metadata_browser: RefCell::new(None),
        });
        ui.add_columns(&table);
        for (button, action) in [
            (&ui.open, "open"),
            (&empty_open, "open"),
            (&add, "subtitle"),
            (&ui.save, "export"),
        ] {
            let weak = Rc::downgrade(&ui);
            button.connect_clicked(move |_| {
                if let Some(ui) = weak.upgrade() {
                    ui.choose(action);
                }
            });
        }
        let weak = Rc::downgrade(&ui);
        metadata_search.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.show_metadata_browser(ImportMode::MetadataAndArtwork);
            }
        });
        let weak = Rc::downgrade(&ui);
        artwork_search.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.show_metadata_browser(ImportMode::ArtworkOnly);
            }
        });
        let weak = Rc::downgrade(&ui);
        ui.cancel_button.connect_clicked(move |_| {
            if let Some(ui) = weak.upgrade() {
                ui.request_cancel();
            }
        });
        for (key, entry) in &ui.fields {
            let key = *key;
            let weak = Rc::downgrade(&ui);
            entry.connect_changed(move |entry| {
                if let Some(ui) = weak.upgrade() {
                    if ui.loading.get() {
                        return;
                    }
                    if let Some(doc) = ui.document.borrow_mut().as_mut() {
                        doc.metadata.insert(key.into(), entry.text().into());
                    }
                    ui.mark_dirty();
                }
            });
        }
        let weak = Rc::downgrade(&ui);
        ui.description.buffer().connect_changed(move |buffer| {
            if let Some(ui) = weak.upgrade() {
                if ui.loading.get() {
                    return;
                }
                if let Some(doc) = ui.document.borrow_mut().as_mut() {
                    doc.metadata.insert(
                        "description".into(),
                        buffer
                            .text(&buffer.start_iter(), &buffer.end_iter(), false)
                            .into(),
                    );
                }
                ui.mark_dirty();
            }
        });
        let drop = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        let weak = Rc::downgrade(&ui);
        drop.connect_drop(move |_, value, _, _| {
            if let Some(ui) = weak.upgrade() {
                if ui.busy.get() {
                    return false;
                }
                if let Ok(file) = value.get::<gio::File>()
                    && let Some(path) = file.path()
                {
                    ui.open_or_import(path);
                    return true;
                }
            }
            false
        });
        ui.window.add_controller(drop);
        for (name, accel) in [
            ("open", "<Primary>o"),
            ("subtitle", "<Primary>i"),
            ("export", "<Primary><Shift>s"),
            ("metadata", "<Primary>m"),
            ("artwork", "<Primary><Shift>m"),
        ] {
            let action = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(&ui);
            action.connect_activate(move |_, _| {
                if let Some(ui) = weak.upgrade() {
                    match name {
                        "metadata" => ui.show_metadata_browser(ImportMode::MetadataAndArtwork),
                        "artwork" => ui.show_metadata_browser(ImportMode::ArtworkOnly),
                        _ => ui.choose(name),
                    }
                }
            });
            ui.window.add_action(&action);
            app.set_accels_for_action(&format!("win.{name}"), &[accel]);
        }
        let weak = Rc::downgrade(&ui);
        ui.window.connect_close_request(move |_| {
            let Some(ui) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if ui.closing.get() && !ui.busy.get() {
                return glib::Propagation::Proceed;
            }
            if ui.busy.get() {
                if ui.cancel.borrow().is_some() {
                    ui.closing.set(true);
                    ui.request_cancel();
                } else {
                    ui.status
                        .set_text("Attendi il completamento dell’analisi del file.");
                }
                return glib::Propagation::Stop;
            }
            if ui.dirty.get() {
                ui.confirm(
                    "Chiudere senza esportare?",
                    "Le modifiche non esportate andranno perse.",
                    "Chiudi",
                    |ui| {
                        ui.closing.set(true);
                        ui.window.close();
                    },
                );
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        ui
    }

    fn confirm(
        self: &Rc<Self>,
        title: &str,
        detail: &str,
        accept: &str,
        action: impl FnOnce(Rc<Self>) + 'static,
    ) {
        let dialog = gtk::AlertDialog::builder()
            .message(title)
            .detail(detail)
            .buttons(["Annulla", accept])
            .cancel_button(0)
            .default_button(0)
            .build();
        let weak = Rc::downgrade(self);
        dialog.choose(Some(&self.window), gio::Cancellable::NONE, move |result| {
            if matches!(result, Ok(1))
                && let Some(ui) = weak.upgrade()
            {
                action(ui);
            }
        });
    }
    fn mark_dirty(&self) {
        self.dirty.set(true);
        self.window
            .set_title(Some("ReelMux · modifiche non esportate"));
        self.refresh_summary();
    }
    fn refresh_summary(&self) {
        if let Some(doc) = self.document.borrow().as_ref() {
            let n = doc.tracks.iter().filter(|t| t.enabled).count();
            self.summary.set_text(&format!(
                "{} · {:.1} MB · {} di {} tracce incluse",
                media::duration_label(doc.duration),
                doc.size as f64 / 1_000_000.0,
                n,
                doc.tracks.len()
            ));
            self.save.set_sensitive(!self.busy.get() && n > 0);
        }
    }
    fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        self.open.set_sensitive(!busy);
        self.editor.set_sensitive(!busy);
        self.progress.set_visible(busy);
        self.progress.set_fraction(0.0);
        self.cancel_button
            .set_visible(busy && self.cancel.borrow().is_some());
        self.cancel_button.set_sensitive(true);
        self.refresh_summary();
    }
    fn error(&self, title: &str, error: impl std::fmt::Display) {
        self.status.set_text(title);
        gtk::AlertDialog::builder()
            .message(title)
            .detail(error.to_string())
            .buttons(["OK"])
            .build()
            .show(Some(&self.window));
    }
    fn show_metadata_browser(self: &Rc<Self>, mode: ImportMode) {
        if self.busy.get() || self.document.borrow().is_none() {
            return;
        }
        if let Some(browser) = self.metadata_browser.borrow().as_ref()
            && browser.window.is_visible()
        {
            browser.window.present();
            return;
        }
        let browser = MetadataBrowser::new(self, mode);
        *self.metadata_browser.borrow_mut() = Some(browser);
    }
    fn apply_imported_metadata(
        self: &Rc<Self>,
        metadata: MetadataResult,
        artwork: Option<Artwork>,
    ) {
        let Some(mut doc) = self.document.borrow().clone() else {
            return;
        };
        doc.apply_metadata(metadata, artwork);
        self.display_document(doc);
        self.mark_dirty();
    }
    fn apply_imported_artwork(self: &Rc<Self>, artwork: Artwork) {
        let Some(mut doc) = self.document.borrow().clone() else {
            return;
        };
        doc.set_artwork(artwork);
        self.display_document(doc);
        self.mark_dirty();
    }
    fn refresh_import_preview(&self, doc: &Document) {
        if let Some(artwork) = doc.artwork.as_ref() {
            let bytes = glib::Bytes::from(&artwork.bytes);
            match gdk::Texture::from_bytes(&bytes) {
                Ok(texture) => {
                    self.artwork.set_paintable(Some(&texture));
                    self.artwork_caption.set_text(&format!(
                        "Locandina da {} · {:.1} MB",
                        artwork.provider,
                        artwork.bytes.len() as f64 / 1_000_000.0
                    ));
                }
                Err(error) => {
                    self.artwork.set_paintable(gdk::Paintable::NONE);
                    self.artwork_caption
                        .set_text(&format!("Anteprima non disponibile: {error}"));
                }
            }
        } else {
            self.artwork.set_paintable(gdk::Paintable::NONE);
            self.artwork_caption.set_text("Nessuna locandina importata");
        }
        let details = [
            ("Provider", "provider"),
            ("Regia", "director"),
            ("Cast", "cast"),
            ("Studio", "studio"),
            ("Rete", "network"),
            ("Crediti", "metadata_attribution"),
        ]
        .into_iter()
        .filter_map(|(label, key)| {
            doc.metadata
                .get(key)
                .filter(|value| !value.is_empty())
                .map(|value| format!("{label}: {value}"))
        })
        .collect::<Vec<_>>()
        .join("\n");
        self.imported_details.set_text(&details);
    }
    fn request_cancel(&self) {
        if let Some(cancel) = self.cancel.borrow().as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.status.set_text("Annullamento in corso…");
        self.cancel_button.set_sensitive(false);
    }
    fn choose(self: &Rc<Self>, action: &'static str) {
        if self.busy.get() || (action != "open" && self.document.borrow().is_none()) {
            return;
        }
        let (title, patterns): (&str, &[&str]) = match action {
            "open" => (
                "Apri un file multimediale",
                &["*.mp4", "*.m4v", "*.mkv", "*.mov", "*.MP4", "*.MKV"],
            ),
            "subtitle" => ("Aggiungi sottotitoli SRT", &["*.srt", "*.SRT"]),
            _ => ("Esporta un nuovo MP4", &["*.mp4"]),
        };
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(title));
        for p in patterns {
            filter.add_pattern(p);
        }
        let all = gtk::FileFilter::new();
        all.set_name(Some("Tutti i file"));
        all.add_pattern("*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        filters.append(&all);
        let dialog = gtk::FileDialog::builder()
            .title(title)
            .filters(&filters)
            .build();
        if action == "export"
            && let Some(doc) = self.document.borrow().as_ref()
        {
            let path = doc.suggested_output();
            dialog.set_initial_name(path.file_name().map(|s| s.to_string_lossy()).as_deref());
            if let Some(parent) = path.parent() {
                dialog.set_initial_folder(Some(&gio::File::for_path(parent)));
            }
        }
        let weak = Rc::downgrade(self);
        let callback = move |result: std::result::Result<gio::File, glib::Error>| {
            if let Some(ui) = weak.upgrade() {
                match result {
                    Ok(file) => {
                        if let Some(mut path) = file.path() {
                            match action {
                                "open" => ui.request_open(path),
                                "subtitle" => ui.import_subtitle(path),
                                _ => {
                                    if path.extension().is_none() {
                                        path.set_extension("mp4");
                                    }
                                    ui.start_export(path);
                                }
                            }
                        }
                    }
                    Err(error) if !error.matches(gtk::DialogError::Dismissed) => {
                        ui.error("Selezione del file non riuscita", error)
                    }
                    _ => (),
                }
            }
        };
        if action == "export" {
            dialog.save(Some(&self.window), gio::Cancellable::NONE, callback);
        } else {
            dialog.open(Some(&self.window), gio::Cancellable::NONE, callback);
        }
    }
    fn open_or_import(self: &Rc<Self>, path: PathBuf) {
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("srt"))
            && self.document.borrow().is_some()
        {
            self.import_subtitle(path);
        } else {
            self.request_open(path);
        }
    }
    fn request_open(self: &Rc<Self>, path: PathBuf) {
        if self.busy.get() {
            return;
        }
        if !self.dirty.get() {
            self.load(path);
            return;
        }
        self.confirm(
            "Aprire un altro file?",
            "Le modifiche non esportate al documento attuale andranno perse.",
            "Apri",
            move |ui| ui.load(path),
        );
    }
    fn load(self: &Rc<Self>, path: PathBuf) {
        self.work_document("Analisi del file…", false, move || Document::open(&path));
    }
    fn import_subtitle(self: &Rc<Self>, path: PathBuf) {
        let Some(mut doc) = self.document.borrow().clone() else {
            return;
        };
        self.work_document("Importazione dei sottotitoli…", true, move || {
            doc.add_subtitle(&path, "und")?;
            Ok(doc)
        });
    }
    fn work_document(
        self: &Rc<Self>,
        message: &str,
        edited: bool,
        work: impl FnOnce() -> Result<Document> + Send + 'static,
    ) {
        if self.busy.get() {
            return;
        }
        self.set_busy(true);
        self.status.set_text(message);
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(work());
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            ui.progress.pulse();
            match rx.try_recv() {
                Ok(result) => {
                    ui.set_busy(false);
                    match result {
                        Ok(doc) => {
                            ui.display_document(doc);
                            if edited {
                                ui.mark_dirty();
                                ui.status.set_text(
                                    "Sottotitoli aggiunti. Imposta la lingua nella tabella.",
                                );
                            }
                        }
                        Err(error) => ui.error("Impossibile leggere il file", format!("{error:#}")),
                    }
                    glib::ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    ui.set_busy(false);
                    ui.error(
                        "Analisi interrotta",
                        "Il processo di analisi è terminato inaspettatamente.",
                    );
                    glib::ControlFlow::Break
                }
                _ => glib::ControlFlow::Continue,
            }
        });
    }
    fn display_document(&self, doc: Document) {
        self.loading.set(true);
        self.filename
            .set_text(&doc.path.file_name().unwrap_or_default().to_string_lossy());
        self.filename
            .set_tooltip_text(Some(&doc.path.to_string_lossy()));
        for (key, entry) in &self.fields {
            entry.set_text(doc.metadata.get(*key).map(String::as_str).unwrap_or(""));
        }
        self.description.buffer().set_text(
            doc.metadata
                .get("description")
                .map(String::as_str)
                .unwrap_or(""),
        );
        self.chapters.set_text(&format!(
            "{} capitoli originali da conservare",
            doc.chapters.len()
        ));
        self.refresh_import_preview(&doc);
        let unsupported = doc.tracks.iter().any(|t| t.unsupported.is_some());
        let count = doc.tracks.len();
        *self.document.borrow_mut() = Some(doc);
        self.model.remove_all();
        for i in 0..count {
            self.model.append(&glib::BoxedAnyObject::new(i));
        }
        self.stack.set_visible_child_name("editor");
        self.loading.set(false);
        self.dirty.set(false);
        self.window.set_title(Some("ReelMux"));
        self.refresh_summary();
        self.status.set_text(if unsupported {
            "Alcune tracce non supportate sono escluse. Passa sul codec per i dettagli."
        } else {
            "Modifica le tracce e i metadati, poi esporta una nuova copia MP4."
        });
    }
    fn start_export(self: &Rc<Self>, path: PathBuf) {
        if self.busy.get() {
            return;
        }
        let Some(doc) = self.document.borrow().clone() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        *self.cancel.borrow_mut() = Some(cancel.clone());
        self.set_busy(true);
        self.status.set_text("Preparazione dell’esportazione…");
        enum Event {
            Update(ExportEvent),
            Done(Result<()>),
        }
        let (tx, rx) = mpsc::channel();
        let output = path.clone();
        thread::spawn(move || {
            let result = media::export(&doc, &output, &cancel, |e| {
                let _ = tx.send(Event::Update(e));
            });
            let _ = tx.send(Event::Done(result));
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(60), move || {
            let Some(ui) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            loop {
                match rx.try_recv() {
                    Ok(Event::Update(ExportEvent::Stage(s))) => ui.status.set_text(&s),
                    Ok(Event::Update(ExportEvent::Progress(Some(p)))) => {
                        ui.progress.set_fraction(p)
                    }
                    Ok(Event::Update(ExportEvent::Progress(None))) => ui.progress.pulse(),
                    Ok(Event::Done(result)) => {
                        let cancelled = ui
                            .cancel
                            .borrow()
                            .as_ref()
                            .is_some_and(|c| c.load(Ordering::Relaxed));
                        *ui.cancel.borrow_mut() = None;
                        ui.set_busy(false);
                        match result {
                            Ok(()) => {
                                ui.dirty.set(false);
                                ui.window.set_title(Some("ReelMux"));
                                ui.status.set_text(&format!(
                                    "Esportazione completata: {}",
                                    path.display()
                                ));
                            }
                            Err(_) if cancelled => ui.status.set_text(
                                "Esportazione annullata. Nessun file di destinazione creato.",
                            ),
                            Err(error) => {
                                ui.error("Esportazione non riuscita", format!("{error:#}"))
                            }
                        }
                        if ui.closing.get() {
                            ui.window.close();
                        }
                        return glib::ControlFlow::Break;
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        *ui.cancel.borrow_mut() = None;
                        ui.set_busy(false);
                        ui.error(
                            "Esportazione interrotta",
                            "Il processo è terminato inaspettatamente.",
                        );
                        return glib::ControlFlow::Break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                }
            }
            glib::ControlFlow::Continue
        });
    }
    fn row(item: &gtk::ListItem) -> Option<usize> {
        item.item()
            .and_downcast::<glib::BoxedAnyObject>()
            .map(|o| *o.borrow::<usize>())
    }
    fn track(&self, i: usize) -> Option<Track> {
        self.document.borrow().as_ref()?.tracks.get(i).cloned()
    }
    fn change_track(&self, i: usize, update: impl FnOnce(&mut Track) -> bool) {
        let changed = self
            .document
            .borrow_mut()
            .as_mut()
            .and_then(|d| d.tracks.get_mut(i))
            .is_some_and(update);
        if changed {
            self.mark_dirty();
        }
    }
    fn add_columns(self: &Rc<Self>, table: &gtk::ColumnView) {
        for (title, field) in [
            ("Usa", "enabled"),
            ("Traccia", "track"),
            ("Nome", "title"),
            ("Lingua", "language"),
            ("Uscita", "mode"),
            ("Predef.", "default"),
            ("Forzati", "forced"),
        ] {
            let factory = gtk::SignalListItemFactory::new();
            let weak = Rc::downgrade(self);
            factory.connect_setup(move |_, object| {
                let item = object.downcast_ref::<gtk::ListItem>().unwrap();
                match field {
                    "enabled" | "default" | "forced" => {
                        let check = gtk::CheckButton::new();
                        check.set_halign(gtk::Align::Center);
                        check.set_tooltip_text(Some(title));
                        let weak = weak.clone();
                        let item_weak = item.downgrade();
                        check.connect_toggled(move |check| {
                            if let (Some(ui), Some(item)) = (weak.upgrade(), item_weak.upgrade())
                                && let Some(i) = Self::row(&item)
                            {
                                ui.change_track(i, |t| {
                                    let v = match field {
                                        "enabled" => &mut t.enabled,
                                        "default" => &mut t.default,
                                        _ => &mut t.forced,
                                    };
                                    let changed = *v != check.is_active();
                                    *v = check.is_active();
                                    changed
                                });
                            }
                        });
                        item.set_child(Some(&check));
                    }
                    "track" => {
                        let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
                        b.set_margin_top(12);
                        b.set_margin_bottom(12);
                        b.append(&label("", "track-title"));
                        b.append(&label("", "muted"));
                        item.set_child(Some(&b));
                    }
                    "mode" => {
                        let dropdown = gtk::DropDown::from_strings(&["Copia", "AAC"]);
                        let weak = weak.clone();
                        let item_weak = item.downgrade();
                        dropdown.connect_selected_notify(move |dropdown| {
                            if let (Some(ui), Some(item)) = (weak.upgrade(), item_weak.upgrade())
                                && let Some(i) = Self::row(&item)
                            {
                                ui.change_track(i, |t| {
                                    if t.kind != "audio" {
                                        return false;
                                    }
                                    let mode = if dropdown.selected() == 1 {
                                        AudioMode::Aac
                                    } else {
                                        AudioMode::Copy
                                    };
                                    let changed = t.audio_mode != mode;
                                    t.audio_mode = mode;
                                    changed
                                });
                            }
                        });
                        let box_ = gtk::Box::new(gtk::Orientation::Vertical, 0);
                        box_.set_valign(gtk::Align::Center);
                        box_.append(&dropdown);
                        box_.append(&label("", "muted"));
                        item.set_child(Some(&box_));
                    }
                    _ => {
                        let entry = gtk::Entry::new();
                        entry.set_has_frame(false);
                        entry.set_width_chars(if field == "language" { 5 } else { 12 });
                        entry.set_tooltip_text(Some(if field == "language" {
                            "Codice: it / ita, en / eng, de / deu…"
                        } else {
                            "Nome della traccia"
                        }));
                        if field == "language" {
                            entry.set_max_length(3);
                        }
                        let weak = weak.clone();
                        let item_weak = item.downgrade();
                        entry.connect_changed(move |entry| {
                            if let (Some(ui), Some(item)) = (weak.upgrade(), item_weak.upgrade())
                                && let Some(i) = Self::row(&item)
                            {
                                let text = entry.text().to_string();
                                if field == "language" {
                                    if language::normalize(&text).is_none() {
                                        entry.add_css_class("error");
                                    } else {
                                        entry.remove_css_class("error");
                                    }
                                }
                                ui.change_track(i, |t| {
                                    let v = if field == "language" {
                                        &mut t.language
                                    } else {
                                        &mut t.title
                                    };
                                    let changed = *v != text;
                                    *v = text;
                                    changed
                                });
                            }
                        });
                        item.set_child(Some(&entry));
                    }
                }
            });
            let weak = Rc::downgrade(self);
            factory.connect_bind(move |_, object| {
                let item = object.downcast_ref::<gtk::ListItem>().unwrap();
                if let Some(ui) = weak.upgrade()
                    && let Some(t) = Self::row(item).and_then(|i| ui.track(i))
                {
                    match field {
                        "enabled" | "default" | "forced" => {
                            let check = item.child().and_downcast::<gtk::CheckButton>().unwrap();
                            check.set_sensitive(
                                t.unsupported.is_none()
                                    && (field != "forced" || t.kind == "subtitle"),
                            );
                            check.set_active(match field {
                                "enabled" => t.enabled,
                                "default" => t.default,
                                _ => t.forced,
                            });
                        }
                        "track" => {
                            let b = item.child().and_downcast::<gtk::Box>().unwrap();
                            let first = b.first_child().and_downcast::<gtk::Label>().unwrap();
                            let second = first.next_sibling().and_downcast::<gtk::Label>().unwrap();
                            let kind = match t.kind.as_str() {
                                "video" => "Video",
                                "audio" => "Audio",
                                "subtitle" => "Sottotitoli",
                                _ => "Dati",
                            };
                            first.set_text(&format!("{kind} · {}", t.codec.to_uppercase()));
                            second.set_text(&t.details);
                            b.set_tooltip_text(t.unsupported.as_deref().or(Some(&t.details)));
                        }
                        "mode" => {
                            let b = item.child().and_downcast::<gtk::Box>().unwrap();
                            let dropdown = b.first_child().and_downcast::<gtk::DropDown>().unwrap();
                            let text = dropdown
                                .next_sibling()
                                .and_downcast::<gtk::Label>()
                                .unwrap();
                            dropdown.set_visible(t.kind == "audio");
                            text.set_visible(t.kind != "audio");
                            text.set_text(t.operation());
                            dropdown.set_selected(if t.audio_mode == AudioMode::Aac {
                                1
                            } else {
                                0
                            });
                        }
                        _ => {
                            let entry = item.child().and_downcast::<gtk::Entry>().unwrap();
                            entry.set_text(if field == "language" {
                                &t.language
                            } else {
                                &t.title
                            });
                        }
                    }
                }
            });
            let column = gtk::ColumnViewColumn::new(Some(title), Some(factory));
            column.set_resizable(true);
            if field == "title" {
                column.set_expand(true);
            }
            table.append_column(&column);
        }
    }
}

pub fn run(path: Option<PathBuf>) -> Result<()> {
    let app = gtk::Application::builder()
        .application_id("io.github.nahime0.ReelMux")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let current: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if let Some(ui) = current.borrow().as_ref() {
            ui.window.present();
            return;
        }
        let provider = gtk::CssProvider::new();
        provider.load_from_data(include_str!("../data/style.css"));
        if let Some(display) = gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        let ui = Ui::new(app);
        ui.window.present();
        if let Some(path) = path.clone() {
            ui.load(path);
        }
        *current.borrow_mut() = Some(ui);
    });
    let status = app.run_with_args::<&str>(&[]);
    anyhow::ensure!(
        status == glib::ExitCode::SUCCESS,
        "L’applicazione si è chiusa con un errore"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pump_until(mut condition: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !condition() {
            while glib::MainContext::default().pending() {
                glib::MainContext::default().iteration(false);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "GUI operation timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn descendants(widget: &gtk::Widget) -> Vec<gtk::Widget> {
        let mut result = vec![widget.clone()];
        let mut next = widget.first_child();
        while let Some(child) = next {
            result.extend(descendants(&child));
            next = child.next_sibling();
        }
        result
    }

    #[test]
    #[ignore = "requires a graphical session, GTK and FFmpeg"]
    fn gui_roundtrip() {
        gtk::init().unwrap();
        let app = gtk::Application::builder()
            .application_id("io.github.nahime0.ReelMux.SmokeTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(gio::Cancellable::NONE).unwrap();
        let provider = gtk::CssProvider::new();
        provider.load_from_data(include_str!("../data/style.css"));
        gtk::style_context_add_provider_for_display(
            &gdk::Display::default().unwrap(),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let dir = tempfile::tempdir().unwrap();
        let result = std::process::Command::new("python")
            .arg("scripts/create_demo.py")
            .arg(dir.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let poster = dir.path().join("poster.jpg");
        let poster_result = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=0x7759ce:s=300x450",
                "-frames:v",
                "1",
            ])
            .arg(&poster)
            .output()
            .unwrap();
        assert!(poster_result.status.success());
        let ui = Ui::new(&app);
        ui.window.present();
        ui.load(dir.path().join("Viaggio-notturno.mkv"));
        pump_until(|| !ui.busy.get() && ui.document.borrow().is_some());
        ui.fields
            .iter()
            .find(|(key, _)| *key == "title")
            .unwrap()
            .1
            .set_text("Viaggio notturno - Prova GUI");
        assert!(ui.dirty.get());
        ui.import_subtitle(dir.path().join("Italiano.srt"));
        pump_until(|| !ui.busy.get());
        pump_until(|| {
            descendants(ui.window.upcast_ref())
                .iter()
                .filter(|w| w.is::<gtk::CheckButton>())
                .count()
                == 12
        });
        let widgets = descendants(ui.window.upcast_ref());
        let entries: Vec<_> = widgets
            .iter()
            .filter_map(|w| w.downcast_ref::<gtk::Entry>())
            .filter(|w| w.tooltip_text().is_some_and(|s| s.contains("it / ita")))
            .collect();
        entries.last().unwrap().set_text("it");
        let include: Vec<_> = widgets
            .iter()
            .filter_map(|w| w.downcast_ref::<gtk::CheckButton>())
            .filter(|w| w.tooltip_text().as_deref() == Some("Usa"))
            .collect();
        include[2].set_active(false);
        let forced: Vec<_> = widgets
            .iter()
            .filter_map(|w| w.downcast_ref::<gtk::CheckButton>())
            .filter(|w| w.tooltip_text().as_deref() == Some("Forzati"))
            .collect();
        forced.last().unwrap().set_active(true);
        assert_eq!(ui.document.borrow().as_ref().unwrap().tracks.len(), 4);
        assert_eq!(
            ui.document.borrow().as_ref().unwrap().metadata["title"],
            "Viaggio notturno - Prova GUI"
        );
        ui.show_metadata_browser(ImportMode::MetadataAndArtwork);
        let browser = ui.metadata_browser.borrow().as_ref().unwrap().clone();
        assert_eq!(browser.provider.selected(), 0);
        assert_eq!(browser.kind.selected(), 0);
        assert_eq!(browser.term.text(), "Viaggio notturno - Prova GUI");
        assert!(!browser.season.is_sensitive());
        browser.kind.set_selected(1);
        assert!(browser.season.is_sensitive());
        let choices = vec![
            ArtworkCandidate {
                provider: Provider::AppleTv,
                url: "https://example.test/poster.jpg".into(),
                thumbnail_url: "https://example.test/poster-small.jpg".into(),
                label: "Poster".into(),
            },
            ArtworkCandidate {
                provider: Provider::AppleTv,
                url: "https://example.test/wide.jpg".into(),
                thumbnail_url: "https://example.test/wide-small.jpg".into(),
                label: "Poster panoramico".into(),
            },
        ];
        browser.display_artworks(
            MetadataResult {
                provider: Provider::AppleTv,
                fields: std::collections::BTreeMap::new(),
                artwork_url: Some(choices[0].url.clone()),
                artworks: choices.clone(),
                source_url: None,
                attribution: Provider::AppleTv.attribution().into(),
            },
            choices.into_iter().map(|choice| (choice, None)).collect(),
        );
        assert_eq!(browser.artwork_choices.borrow().len(), 2);
        assert_eq!(browser.selected.get(), Some(0));
        assert_eq!(
            browser.import.label().as_deref(),
            Some("Importa metadati e locandina")
        );
        if let Some(path) = std::env::var_os("REELMUX_TEST_BROWSER_SCREENSHOT") {
            let mut frames = 0;
            pump_until(|| {
                frames += 1;
                frames > 20
            });
            let paintable = gtk::WidgetPaintable::new(Some(&browser.window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(
                &snapshot,
                browser.window.width() as f64,
                browser.window.height() as f64,
            );
            let node = snapshot.to_node().expect("browser must render");
            browser
                .window
                .renderer()
                .unwrap()
                .render_texture(&node, None)
                .save_to_png(path)
                .unwrap();
        }
        browser.window.close();
        let artwork_browser = MetadataBrowser::new(&ui, ImportMode::ArtworkOnly);
        assert_eq!(artwork_browser.mode, ImportMode::ArtworkOnly);
        assert_eq!(artwork_browser.term.text(), "Viaggio notturno - Prova GUI");
        assert_eq!(
            artwork_browser.import.label().as_deref(),
            Some("Mostra locandine")
        );
        artwork_browser.window.close();
        let metadata_before = ui.document.borrow().as_ref().unwrap().metadata.clone();
        let poster_bytes = std::fs::read(&poster).unwrap();
        ui.apply_imported_artwork(Artwork {
            bytes: poster_bytes.clone(),
            media_type: "image/jpeg".into(),
            source_url: "https://example.test/poster.jpg".into(),
            provider: Provider::AppleTv,
        });
        assert_eq!(
            ui.document.borrow().as_ref().unwrap().metadata,
            metadata_before
        );
        assert_eq!(
            ui.document
                .borrow()
                .as_ref()
                .unwrap()
                .artwork
                .as_ref()
                .unwrap()
                .bytes,
            poster_bytes
        );
        // Capture only our widget tree, independent of overlapping desktop windows.
        if let Some(path) = std::env::var_os("REELMUX_TEST_SCREENSHOT") {
            let mut frames = 0;
            pump_until(|| {
                frames += 1;
                frames > 20
            });
            let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
            let snapshot = gtk::Snapshot::new();
            paintable.snapshot(
                &snapshot,
                ui.window.width() as f64,
                ui.window.height() as f64,
            );
            let node = snapshot.to_node().expect("window must render");
            ui.window
                .renderer()
                .unwrap()
                .render_texture(&node, None)
                .save_to_png(path)
                .unwrap();
        }
        let output = dir.path().join("gui-export.mp4");
        ui.start_export(output.clone());
        pump_until(|| !ui.busy.get());
        assert!(
            ui.status.text().starts_with("Esportazione completata"),
            "{}",
            ui.status.text()
        );
        assert!(!ui.dirty.get());
        let doc = Document::open(&output).unwrap();
        assert_eq!(doc.metadata["title"], "Viaggio notturno - Prova GUI");
        assert_eq!(doc.tracks.iter().filter(|t| t.kind == "audio").count(), 1);
        let subtitle = doc.tracks.iter().find(|t| t.kind == "subtitle").unwrap();
        assert_eq!(subtitle.language, "ita");
        assert!(subtitle.forced);
        assert_eq!(doc.chapters.len(), 2);
        assert!(doc.artwork.is_some());
        ui.window.close();
    }
}

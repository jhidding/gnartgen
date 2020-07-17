// ~\~ language=Rust filename=src/main.rs
// ~\~ begin <<lit/index.md|src/main.rs>>[0]
extern crate pretty_env_logger;
extern crate gtk;
extern crate gio;
extern crate sourceview;
extern crate rusqlite;
extern crate glib;
extern crate pango;

use gtk::prelude::*;

#[derive(Debug)]
pub enum Error {
    SQL(rusqlite::Error),
    User(String),
}

type Result<T, E=Error> = std::result::Result<T, E>;

mod ui {
    use std::collections::HashMap;
    use gtk::prelude::*;
    use glib::clone;
    use sourceview::prelude::*;
    use std::sync::mpsc::{Sender};
    use std::path::PathBuf;

    use super::state;
    use super::{Result,Error};

    pub enum Msg {
        SetFilename(PathBuf),
        ClearItems,
        NewItem(ItemInfo),
        SetSource(String),
    }

    pub struct ItemInfo {
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub thumbnail: (),
    }

    pub struct App {
        builder:    gtk::Builder,
        window:     gtk::Window,
        code_view:  sourceview::View,
        header_bar: gtk::HeaderBar,
        item_list:  gtk::ListBox,
    }

    impl App {
        pub fn new() -> App {
            log::debug!("building Glade components");
            let glade_src = std::include_str!("../data/gnartgen.glade");
            let builder = gtk::Builder::from_string(glade_src);
            let window: gtk::Window = builder.get_object("window").unwrap();
            let header_bar: gtk::HeaderBar = builder.get_object("header").unwrap();
            let code_view: sourceview::View = builder.get_object("code_view").unwrap();
            let item_list: gtk::ListBox = builder.get_object("item_list").unwrap();

            log::debug!("creating code buffer");
            init_code_buffer(&code_view);
            window.show_all();

            App {
                builder: builder, window: window, code_view: code_view,
                header_bar: header_bar, item_list: item_list
            }
        }

        pub fn clear_items(&self)
        {
            self.item_list.foreach(|x| self.item_list.remove(x));
        }


        pub fn connect(&self, tx_state: Sender<state::Msg>) -> Result<()>
        {
            self.builder.connect_signals(|_, signal_name| {
                match signal_name {
                    "on_window_destroy" => {
                        log::debug!("Connecting on_window_destroy signal");
                        Box::new(|_| {
                            log::info!("Bye!");
                            gtk::main_quit(); None
                        })
                    }
                    "open_button_clicked" => {
                        log::debug!("Connecting on_button_clicked signal");
                        let _window = self.window.clone();
                        Box::new(clone!(@strong tx_state => move |_| {
                            let path = select_file_dialog(&_window);
                            if path.is_none() { return None; }
                            tx_state.send(state::Msg::Open(path.unwrap())).unwrap();
                            None
                        }))
                    }
                    "add_item_clicked" => {
                        Box::new(clone!(@strong tx_state => move |_| {
                            tx_state.send(state::Msg::NewItem).unwrap(); None }))
                    }
                    "item_select" => {
                        let buffer = self.code_view.get_buffer().unwrap();
                        let list_box = self.item_list.clone();
                        Box::new(clone!(@strong tx_state => move |x| {
                            let (start, end) = buffer.get_bounds();
                            let text = buffer.get_text(&start, &end, false)
                                .unwrap().to_string();
                            tx_state.send(state::Msg::StoreCode(text)).unwrap();
                            let widget = list_box.get_selected_row().unwrap().get_children()[0].clone();
                            let id: i64;
                            unsafe { id = *widget.get_data("db_id").unwrap() };
                            tx_state.send(state::Msg::SelectItem(id)).unwrap();
                            None }))
                    }
                    _ => {
                        log::debug!("Not connecting to {} signal", signal_name);
                        Box::new(|_| { None })
                    }
                }
            });
            Ok(())
        }

        pub fn handle(&self, msg: Msg) -> glib::Continue {
            match msg {
                Msg::SetFilename(path) => {
                    self.header_bar.set_title(path.file_name().and_then(|s| s.to_str()));
                    self.header_bar.set_subtitle(path.parent().and_then(|p| p.to_str()));
                }
                Msg::ClearItems => {
                    self.clear_items()
                }
                Msg::NewItem(info) => {
                    let widget = create_card(info.name, info.description);
                    unsafe { widget.set_data("db_id", info.id); }
                    self.item_list.insert(&widget, -1);
                    self.item_list.show_all();
                }
                Msg::SetSource(text) => {
                    let buffer = self.code_view.get_buffer().unwrap();
                    buffer.set_text(text.as_str());
                }
            }
            glib::Continue(true)
        }
    }

    fn init_code_buffer(view: &sourceview::View) -> sourceview::Buffer {
        use sourceview::{Buffer,StyleSchemeManager,LanguageManager};
        let language_manager = LanguageManager::get_default()
            .expect("Default language manager");
        let scheme_language = language_manager.get_language("scheme")
            .expect("Scheme language");
        let theme_manager = StyleSchemeManager::get_default()
            .expect("Default style scheme manager");
        let theme = theme_manager.get_scheme("tango")
            .expect("Tango style scheme");
        let code_buffer = Buffer::new_with_language(&scheme_language);
        code_buffer.set_style_scheme(Some(&theme));
        view.set_buffer(Some(&code_buffer));
        code_buffer
    }

    fn create_card(name: String, description: Option<String>) -> impl IsA<gtk::Widget> {
        use gtk::{Orientation,IconSize};
        let outer = gtk::Box::new(Orientation::Vertical, 0);
        let row1 = gtk::Box::new(Orientation::Horizontal, 0);
        let row2 = gtk::Box::new(Orientation::Horizontal, 0);
        let title = gtk::Label::new(Some(name.as_str()));
        let descr = gtk::Label::new(description.as_ref().map(|x| x.as_str()));
        descr.set_single_line_mode(false);
        descr.set_line_wrap(true);
        descr.set_lines(3);
        descr.set_ellipsize(pango::EllipsizeMode::End);
        let thumb = gtk::Image::new();
        let destr = gtk::Button::from_icon_name(Some("edit-delete-symbolic"), IconSize::SmallToolbar);
        outer.pack_start(&row1, false, true, 0);
        outer.pack_start(&row2, false, true, 0);
        row1.pack_start(&title, true, true, 0);
        row1.pack_start(&destr, false, true, 0);
        row2.pack_start(&descr, true, true, 5);
        row2.pack_start(&thumb, false, true, 0);
        outer
    }

    fn select_file_dialog(w: &gtk::Window) -> Option<std::path::PathBuf> {
        let dialog = gtk::FileChooserDialog::with_buttons(
            Some("Open File"), Some(w), gtk::FileChooserAction::Save,
            &[("_Cancel", gtk::ResponseType::Cancel), ("_Open", gtk::ResponseType::Accept)]);
        let result = match dialog.run() {
            gtk::ResponseType::Cancel => None,
            gtk::ResponseType::Accept => dialog.get_filename(),
            _                         => None
        };
        unsafe { dialog.destroy(); }
        result
    }
}

// ~\~ begin <<lit/index.md|open-file>>[0]
mod state {
    use std::path::{Path,PathBuf};
    use rusqlite::{Connection};
    use std::sync::mpsc::{Receiver};
    use super::{Error, Result, ui};
    use std::collections::HashSet;
    use rusqlite::params;

    const SCHEMA : &str = std::include_str!("../data/schema.sqlite");

    fn open_file<P: AsRef<Path>>(path: P) -> Result<Connection> {
        let conn = Connection::open(path).map_err(Error::SQL)?;
        conn.execute_batch(&SCHEMA).map_err(Error::SQL)?;
        Ok(conn)
    }

    fn save_file<P: AsRef<Path>>(conn: &mut Connection, path: P) -> Result<()> {
        use rusqlite::DatabaseName::*;
        use rusqlite::backup::Progress;
        conn.backup(Main, path, None::<fn(Progress)>).map_err(Error::SQL)
    }

    fn new_project() -> Result<Connection> {
        let conn = Connection::open_in_memory().map_err(Error::SQL)?;
        let schema = std::include_str!("../data/schema.sqlite");
        conn.execute_batch(&schema).map_err(Error::SQL)?;
        Ok(conn)
    } 

    pub struct State {
        open_file: Option<PathBuf>,
        conn: Connection,
        active_object: Option<i64>,
    }

    pub enum Msg {
        Open(PathBuf),
        NewItem,
        StoreCode(String),
        SetActiveObject(String),
        SelectItem(i64),
        SetDescription(String),
        SetName(String),
    }

    impl State {
        pub fn new() -> State
        {
            let conn = new_project().unwrap();
            State { open_file: None, conn: conn, active_object: None }
        }

        fn open<P: AsRef<Path>>(&mut self, path: P) -> Result<()>
        {
            let conn = open_file(path.as_ref())?;
            log::info!("Loaded {:}.", path.as_ref().display());
            self.conn = conn;
            Ok(())
        }

        fn save_as(&mut self, path: PathBuf) -> Result<()>
        {
            save_file(&mut self.conn, &path)?;
            self.open(path)
        }

        fn unique_name(&self) -> Result<String>
        {
            let mut stmt = self.conn.prepare("select `name` from `objects`")
                .map_err(Error::SQL)?;
            let existing: HashSet<String> = stmt.query_map(params![], |r| r.get(0))
                .map_err(Error::SQL)?.map(|x| x.unwrap()).collect();
            let default_name: &str = "New Object";
            let mut i = 1;
            loop {
                let name = format!("{} ({})", default_name, i);
                if !existing.contains(&name) {
                    return Ok(name);
                }
                i += 1;
            }
        }

        fn insert_new(&self, name: &String, description: &Option<String>)
            -> Result<i64>
        {
            self.conn.execute(
                "insert into `objects`(`name`, `description`) values (?1, ?2)",
                params![name, description]).map_err(Error::SQL)?;
            Ok(self.conn.last_insert_rowid())
        }

        fn update_name(&self, name: &String) -> Result<()> {
            let id = self.active_object
                .ok_or(Error::User("No object selected.".to_string()))?;
            self.conn.execute(
                "update `objects` set `name` = ?2 where `id` = ?1",
                params![id, name]).map_err(Error::SQL)?;
            Ok(())
        }

        fn update_source(&self, source: &String) -> Result<()> {
            let id = self.active_object
                .ok_or(Error::User("No object selected.".to_string()))?;
            self.conn.execute(
                "update `objects` set `source` = ?2 where `id` = ?1",
                params![id, source]).map_err(Error::SQL)?;
            Ok(())
        }

        fn update_description(&self, descr: &String) -> Result<()> {
            let id = self.active_object
                .ok_or(Error::User("No object selected.".to_string()))?;
            self.conn.execute(
                "update `objects` set `description` = ?2 where `id` = ?1",
                params![id, descr]).map_err(Error::SQL)?;
            Ok(())
        }

        fn select_by_name(&mut self, name: &String) -> Result<()> {
            let mut stmt = self.conn.prepare(
                "select `id` from `objects` where `name` = ?1")
                .map_err(Error::SQL)?;
            let id = stmt.query_row(params![name], |r| r.get(0))
                .map_err(Error::SQL)?;
            self.active_object = Some(id);
            Ok(())
        }

        fn read_source(&mut self, id: i64) -> Result<String> {
            let mut stmt = self.conn.prepare(
                "select `source` from `objects` where `id` = ?1")
                .map_err(Error::SQL)?;
            stmt.query_row(params![id], |r| r.get(0)).map_err(Error::SQL)
        }

        pub fn listen(&mut self, tx_event: glib::Sender<ui::Msg>, rx: Receiver<Msg>) {
            for msg in rx {
                match msg {
                    Msg::Open(path) => {
                        if self.open(&path).is_ok() {
                            self.open_file = Some(path.clone());
                            tx_event.send(ui::Msg::SetFilename(path)).unwrap();
                        }
                    }
                    Msg::NewItem => {
                        let name = self.unique_name().unwrap();
                        let description = Some("Mostly harmless.".to_string());
                        let id = self.insert_new(&name, &description).unwrap();
                        tx_event.send(ui::Msg::NewItem(ui::ItemInfo {
                            id: id, name: name, description: description, thumbnail: () })).unwrap();
                    }
                    Msg::StoreCode(text) => {
                        self.update_source(&text).unwrap_or_else(|e| {
                            log::warn!("{:?}", e); });
                    }
                    Msg::SetActiveObject(obj) => {
                        self.select_by_name(&obj).unwrap_or_else(|e| {
                            log::warn!("{:?}", e); });
                    }
                    Msg::SetDescription(text) => {
                        self.update_description(&text).unwrap_or_else(|e| {
                            log::warn!("{:?}", e); });
                    }
                    Msg::SetName(name) => {
                        self.update_name(&name).unwrap_or_else(|e| {
                            log::warn!("{:?}", e); });
                    }
                    Msg::SelectItem(id) => {
                        self.active_object = Some(id);
                        let source = self.read_source(id).unwrap_or("".to_string());
                        tx_event.send(ui::Msg::SetSource(source)).unwrap();
                    }
                }
            }
        }
    }
}
// ~\~ end

/*    fn set_title(&self) {
        let header: gtk::HeaderBar = self.builder.get_object("header").unwrap();

        header.set_title(self.open_file.as_ref().map(|x| x.as_str()));
    }*/
fn main() {
    use std::sync::mpsc::channel;

    pretty_env_logger::init();
    if gtk::init().is_err() {
        println!("Failed to initialize GTK.");
        return;
    }
    sourceview::View::static_type();

    let (tx_state, rx_state) = channel();
    let (tx_event, rx_event) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);

    let mut state = state::State::new();
    std::thread::spawn(move || { state.listen(tx_event.clone(), rx_state); });

    let app = ui::App::new();
    app.clear_items();
    app.connect(tx_state.clone()).unwrap();
    rx_event.attach(None, move |msg| app.handle(msg));

    gtk::main();
}
// ~\~ end

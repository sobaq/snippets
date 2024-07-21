#![feature(is_none_or, iterator_try_collect)]
use std::rc::Rc;
use eframe::egui::{self, Key};

mod database;
use database::{SearchResult, Snippet};

struct TreeBehaviour;

struct Pane {
    // I'm not sure I like this, but Panes can't access Snippy
    // This could be managed with lifetimes, but it doesn't seem to interact
    // well with eframe.
    db: Rc<database::Connection>,
    snippet: Snippet,

    /// Whether the snippet has been modified in memory
    dirty: bool,
    // Whether the snippet has ever been dirty before.
    // Unmodified snippets are replaced by newly opened ones.
    // previously_dirty: bool,
}

struct Snippy {
    db: Rc<database::Connection>,
    current_search: String,
    search_results: Vec<SearchResult>,

    snippet_tree: egui_tiles::Tree<Pane>,

    // Small hack to auto-focus the search field on start
    first_frame: bool,
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions::default();

    eframe::run_native("Snippy", options, Box::new(|_| Ok(Box::new(Snippy::new().unwrap()))))
}

impl<'a> egui_tiles::Behavior<Pane> for TreeBehaviour {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        let max_title_len = 12;
        let (mark, end) = if pane.dirty { ("* ", 10) } else { ("", 12) };
        let trail = if pane.snippet.name.len() > max_title_len { ".." } else { "" };

        format!("{mark}{}{trail}", &pane.snippet.name[..end]).into()
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _: egui_tiles::TileId, pane: &mut Pane) -> egui_tiles::UiResponse {
        let content_resp = ui.with_layout(
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| ui.text_edit_multiline(&mut pane.snippet.content),
        ).inner;

        if content_resp.changed() {
            pane.dirty = true;
        }

        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::S)) {
            pane.db.save_snippet(&pane.snippet).unwrap();
            pane.dirty = false;
        }

        Default::default()
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        egui_tiles::SimplificationOptions {
            all_panes_must_have_tabs: true,
            ..Default::default()
         }
    }

    fn is_tab_closable(&self, _: &egui_tiles::Tiles<Pane>, _: egui_tiles::TileId) -> bool {
        true
    }
}

impl Snippy {
    fn new() -> anyhow::Result<Self> {
        let new = Self {
            db: Rc::new(database::Connection::open("/tmp/snippets-test.sqlite3")?),
            // db: Rc::new(database::Connection::open_in_memory()?),
            current_search: Default::default(),
            search_results: Default::default(),
            first_frame: true,

            snippet_tree: egui_tiles::Tree::new_tabs("snippet_tree", vec![]),
        };

        Ok(new)
    }

    fn search(&mut self) -> anyhow::Result<()> {
        if self.current_search.trim().is_empty() {
            self.search_results = self.db.recent(25)?;
        } else {
            self.search_results = self.db.search(&self.current_search, 25)?;
        }

        Ok(())
    }
}

impl eframe::App for Snippy {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("search_panel")
            .default_width(225.)
            .show(ctx, |ui|
        {
            let search = ui.text_edit_singleline(&mut self.current_search);
            if self.first_frame {
                search.request_focus();
                self.search().unwrap();
                self.first_frame = false;
            }

            if search.changed() {
                self.search().unwrap();
            }

            for (i, result) in self.search_results.iter().enumerate() {
                let mut frame = egui::Frame::default()
                    .inner_margin(4.0)
                    .begin(ui);
                let response = {
                    frame.content_ui.set_width(frame.content_ui.available_width());
                    frame.content_ui.heading(&result.name);
                    frame.content_ui.label(&result.hint);
    
                    if i % 2 == 0 {
                        frame.frame.fill = frame.content_ui.style().visuals.extreme_bg_color;
                    }
    
                    let response = frame.allocate_space(ui).interact(egui::Sense::click());
                    if response.hovered() {
                        frame.frame.stroke = frame.content_ui.style().visuals.selection.stroke;
                    }

                    response
                };
                frame.paint(ui);

                if response.clicked() {
                    // Returns true if it made a pane active, i.e. this snippet was already open.
                    let already_open = self.snippet_tree.make_active(|_, tile|
                        match tile {
                            egui_tiles::Tile::Pane(s) if s.snippet.id.is_some_and(|id| id == result.id) => true,
                            _ => false,
                        }
                    );

                    if !already_open {
                        let pane = Pane {
                            snippet: self.db.fetch_snippet(result.id).unwrap(),
                            db: Rc::clone(&self.db),
                            dirty: false,
                        };
                        let pane_id = self.snippet_tree.tiles.insert_pane(pane);
    
                        if let Some(root) = self.snippet_tree.root {
                            let tile_count = self.snippet_tree.tiles.len();
                            self.snippet_tree.move_tile_to_container(pane_id, root, tile_count, false);
                        } else {
                            self.snippet_tree.root = Some(self.snippet_tree.tiles.insert_tab_tile(vec![pane_id]));
                        }
                    }
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut behaviour = TreeBehaviour;
            self.snippet_tree.ui(&mut behaviour, ui);
        });
    }
}
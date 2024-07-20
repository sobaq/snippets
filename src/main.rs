#![feature(iterator_try_collect)]
use eframe::egui;

extern {
    fn sqlite3_spellfix_init(
        db: *mut rusqlite::ffi::sqlite3,
        pzErrMsg: *mut *mut std::ffi::c_char,
        pApi: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int;
}

#[derive(Default)]
struct SearchResult {
    id: i64,
    name: String,
    hint: String,
}

#[derive(Default, Clone)]
struct Snippet {
    id: Option<i64>,
    name: String,
    content: String,
}

struct Snippets {
    db: rusqlite::Connection,
    current_search: String,
    results: Vec<SearchResult>,
    selected_snippet_rowid: Option<i64>,
    selected_snippet: Snippet,

    snippet_needs_update: bool,
    // Small hack to auto-focus the search field on start
    first_frame: bool,
}

fn main() {
    let native_options = eframe::NativeOptions::default();
    eframe::run_native("Snippets", native_options, Box::new(|cc| Ok(Box::new(Snippets::new(cc).unwrap())))).unwrap();
}

impl Snippets {
    fn new(_cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        let new = Self {
            db: rusqlite::Connection::open_in_memory()?,
            current_search: Default::default(),
            results: Default::default(),
            selected_snippet_rowid: None,
            selected_snippet: Default::default(),
            snippet_needs_update: false,
            first_frame: true,
        };

        unsafe {
            let _guard = rusqlite::LoadExtensionGuard::new(&new.db)?;
            assert_eq!(sqlite3_spellfix_init(new.db.handle(), std::ptr::null_mut(), std::ptr::null()), rusqlite::ffi::SQLITE_OK);
        };

        new.db.execute_batch("
            CREATE         TABLE IF NOT EXISTS snippets (name TEXT, content TEXT, created_at INTEGER DEFAULT CURRENT_TIMESTAMP);
            CREATE VIRTUAL TABLE IF NOT EXISTS snippets_fts      USING fts5(name, content, content=snippets);
            CREATE VIRTUAL TABLE IF NOT EXISTS snippets_terms    USING fts5vocab(snippets_fts, col);
            CREATE VIRTUAL TABLE IF NOT EXISTS snippets_spellfix USING spellfix1;

            CREATE TRIGGER snippets_fts_before_update BEFORE UPDATE ON snippets BEGIN
                DELETE FROM snippets_fts WHERE rowid=old.rowid;
            END;

            CREATE TRIGGER snippets_fts_before_delete BEFORE DELETE ON snippets BEGIN
                DELETE FROM snippets_fts WHERE rowid=old.rowid;
            END;

            CREATE TRIGGER snippets_after_update AFTER UPDATE ON snippets BEGIN
                INSERT INTO snippets_fts(rowid, name, content)
                SELECT rowid, name, content FROM snippets
                WHERE new.rowid = snippets.rowid;
            END;

            CREATE TRIGGER snippets_after_insert AFTER INSERT ON snippets BEGIN
                INSERT INTO snippets_fts(rowid, name, content)
                SELECT rowid, name, content FROM snippets
                WHERE new.rowid = snippets.rowid;
            END;

            INSERT INTO snippets (name, content) VALUES ('Recent serious system logs', 'doas journalctl -p0..3 -rx');
            INSERT INTO snippets (name, content) VALUES ('Strip video metadata', 'ffmpeg -i $IN -map_metadata -1 -c:v copy -c:a copy $OUT');
            INSERT INTO snippets (name, content) VALUES ('Scale down video', 'ffmpeg -i $IN -s 720x480 -c:a copy $OUT');
            INSERT INTO snippets (name, content) VALUES ('Generate 50 random 6 character strings',
                'for i in $(seq 50); do cat /dev/urandom | tr -dc a-z | head -c6; printf ''\\n''; done');
        ")?;

        Ok(new)
    }

    fn search(&mut self) -> anyhow::Result<()> {
        if self.current_search.trim().is_empty() {
            self.results = self.db.prepare("
                SELECT rowid, name, snippet(snippets_fts, 1, '', '', '..', 1), content FROM snippets_fts LIMIT 25
            ")?.query_map([], |row| Ok(SearchResult { id: row.get(0)?, name: row.get(1)?, hint: row.get(2)?, }))?
               .try_collect()?;

            return Ok(());
        }

        let mut corrected_query = Vec::new();
        for term in self.current_search.split_whitespace() {
            let spell_fixed: String = self.db.query_row(
                "SELECT word FROM snippets_spellfix WHERE word MATCH ?1 AND top = 1",
                (term,),
                |row| row.get(0)
            ).unwrap_or_else(|_| term.to_string());
            corrected_query.push(spell_fixed);
        }
        let corrected_query = corrected_query.join(" ");

        self.results = self.db.prepare("
            SELECT rowid, name, snippet(snippets_fts, 1, '', '', '..', 8), content FROM snippets_fts WHERE content MATCH ?1 OR name MATCH ?1 ORDER BY bm25(snippets_fts) LIMIT 25
        ")?.query_map((corrected_query,), |row| Ok(SearchResult { id: row.get(0)?, name: row.get(1)?, hint: row.get(2)?, }))?
           .try_collect()?;

        Ok(())
    }

    fn set_snippet(&mut self) -> anyhow::Result<()> {
        self.selected_snippet = self.db.query_row("
            SELECT ?1, name, content FROM snippets WHERE rowid = ?1;
        ", [self.selected_snippet_rowid], |row| Ok(Snippet { id: row.get(0)?, name: row.get(1)?, content: row.get(2)?, }))?;

        Ok(())
    }

    fn save_snippet(&mut self) -> anyhow::Result<()> {
        // update existing snippet
        if let Some(rowid) = self.selected_snippet.id {
            self.db.execute("
                UPDATE snippets SET name = ?1, content = ?2 WHERE rowid = ?3
            ", (&self.selected_snippet.name, &self.selected_snippet.content, rowid))?;
        } /* new snippet */ else {
            self.selected_snippet.id = Some(self.db.query_row("
                INSERT INTO snippets (name, content) VALUES (?1, ?2) RETURNING rowid
            ", (&self.selected_snippet.name, &self.selected_snippet.content), |row| row.get(0))?);
        }

        Ok(())
    }
}

impl eframe::App for Snippets {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.selected_snippet.name.is_empty() {
            self.selected_snippet.name = String::from("Untitled Snippet");
        }

        if self.selected_snippet_rowid.is_some() && self.snippet_needs_update {
            self.snippet_needs_update = false;
            self.set_snippet().unwrap();
        }

        egui::SidePanel::left("search_panel").show(ctx, |ui| {
            let search = ui.text_edit_singleline(&mut self.current_search);
            if self.first_frame {
                search.request_focus();
                self.search().unwrap();
                self.first_frame = false;
            }

            if search.changed() {
                self.search().unwrap();
            }

            for (i, result) in self.results.iter().enumerate() {
                let mut frame = egui::Frame::default()
                    .inner_margin(4.0)
                    .begin(ui);
                {
                    frame.content_ui.set_width(frame.content_ui.available_width());
                    frame.content_ui.heading(&result.name);
                    frame.content_ui.label(&result.hint);
                }

                if i % 2 == 0 {
                    frame.frame.fill = frame.content_ui.style().visuals.extreme_bg_color;
                }

                let response = frame.allocate_space(ui).interact(egui::Sense::click());
                if response.hovered() {
                    frame.frame.stroke = frame.content_ui.style().visuals.selection.stroke;
                }

                if response.clicked() {
                    self.selected_snippet_rowid = Some(result.id);
                    self.snippet_needs_update = true;
                }

                if self.selected_snippet_rowid.is_some_and(|id| id == result.id) {
                    frame.frame.fill = frame.content_ui.style().visuals.selection.bg_fill;
                }

                frame.paint(ui);
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let title_resp = ui.text_edit_singleline(&mut self.selected_snippet.name);
            let content_resp = ui.with_layout(
                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                |ui| ui.text_edit_multiline(&mut self.selected_snippet.content),
            ).inner;

            if title_resp.changed() || content_resp.changed() {
                self.save_snippet().unwrap()
            }
        });
    }
}
static MIGRATIONS: include_dir::Dir = include_dir::include_dir!("migrations");

extern {
    fn sqlite3_spellfix_init(
        db: *mut rusqlite::ffi::sqlite3,
        pzErrMsg: *mut *mut std::ffi::c_char,
        pApi: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> std::ffi::c_int;
}

pub struct Connection(rusqlite::Connection);

#[derive(Default, Clone)]
pub struct Snippet {
    pub id: Option<i64>,
    pub name: String,
    pub content: String,
}

#[derive(Default)]
pub struct SearchResult {
    pub id: i64,
    pub name: String,
    pub hint: String,
}

impl Connection {
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let mut db = Self(rusqlite::Connection::open_in_memory()?);
        db.apply_migrations()?;

        Ok(db)
    }

    pub fn open<P: AsRef<std::path::Path>>(at: P) -> rusqlite::Result<Self> {
        let mut db = Self(rusqlite::Connection::open(at)?);
        db.apply_migrations()?;

        Ok(db)
    }

    fn apply_migrations(&mut self) -> rusqlite::Result<()> {
        unsafe {
            let _guard = rusqlite::LoadExtensionGuard::new(&self.0)?;
            assert_eq!(
                sqlite3_spellfix_init(self.0.handle(), std::ptr::null_mut(), std::ptr::null()),
                rusqlite::ffi::SQLITE_OK
            );
        };

        self.0.execute_batch("
            CREATE TABLE IF NOT EXISTS migrations (hash BLOB);
        ")?;

        for file in MIGRATIONS.files() {
            if file.path().extension().is_none_or(|name| name != "sql") {
                continue;
            }

            let hash = blake3::hash(file.contents()).to_hex().to_string();
            let migration = file.contents_utf8()
                .expect("include_dir! included a source file containing invalid utf8");

            {
                let transaction = self.0.transaction()?;

                let already_executed: bool = transaction.query_row("
                    SELECT EXISTS (SELECT 1 FROM migrations WHERE hash = ?1)
                ", (&hash,), |row| row.get(0))?;

                if already_executed {
                    continue;
                }

                transaction.execute("
                    INSERT INTO migrations (hash) VALUES (?1);
                ", (&hash,))?;
                transaction.execute_batch(&migration)?;
                transaction.commit()?;
            }
        }

        Ok(())
    }

    /// Fetch a `Snippet` from its `rowid`
    pub fn fetch_snippet(&self, id: i64) -> rusqlite::Result<Snippet> {
        self.0.query_row("
            SELECT name, content
                FROM snippets
                WHERE rowid = ?1;
        ", [id], |row| Ok(Snippet { id: Some(id), name: row.get(0)?, content: row.get(1)?, }))
    }

    /// Return up to `limit` `Snippet`s, sorted by most recently created
    pub fn recent(&self, limit: u16) -> rusqlite::Result<Vec<SearchResult>> {
        self.0.prepare_cached("
            SELECT rowid, name, replace(substr(content, 0, 64) || '..', '\n', ' ')
                FROM snippets
                ORDER BY created_at DESC
                LIMIT ?1
        ")?.query_map((limit,), |row| Ok(SearchResult { id: row.get(0)?, name: row.get(1)?, hint: row.get(2)?, }))?
           .try_collect()
    }

    /// Inserts or updates `snippet`, returning its `rowid`.
    pub fn save_snippet(&self, snippet: &Snippet) -> rusqlite::Result<i64> {
        if let Some(rowid) = snippet.id {
            self.0.execute("
                UPDATE snippets
                    SET name = ?1, content = ?2
                    WHERE rowid = ?3
            ", (&snippet.name, &snippet.content, rowid))?;

            Ok(rowid)
        } else {
            self.0.query_row("
                INSERT INTO snippets (name, content)
                    VALUES (?1, ?2)
                    RETURNING rowid
            ", (&snippet.name, &snippet.content), |row| row.get(0))
        }
    }

    /// Search for `Snippet`s with a name or contents matching `query`, returning up to `limit` results
    pub fn search(&self, query: &str, limit: u16) -> rusqlite::Result<Vec<SearchResult>> {
        let corrected = self.fix_search(query)?;

        self.0.prepare_cached("
            SELECT rowid, name, replace(snippet(snippets_fts, 1, '', '', '..', 8), '\n', ' ')
                FROM snippets_fts
                WHERE  content LIKE ?1
                    OR name    LIKE ?1 
                ORDER BY bm25(snippets_fts)
                LIMIT ?2
        ")?.query_map((corrected, limit),
            |row| Ok(SearchResult { id: row.get(0)?, name: row.get(1)?, hint: row.get(2)?, }))?
        .try_collect()
    }

    /// Attempts to improve search results by modifying the search query.
    /// The query is split on spaces into words; if that word exists as a
    /// substring of any term in any snippet, it is used verbatim. Otherwise,
    /// a correction is attempted with `spellfix1`.
    /// Every word is wrapped with '%' because by default `fts5` only matches
    /// prefixes.
    fn fix_search(&self, query: &str) -> rusqlite::Result<String> {
        let mut corrected_query = Vec::new();
        for term in query.split_whitespace() {
            let spell_fixed: String = self.0.query_row("
                SELECT ?1 AS word
                    WHERE EXISTS (
                        SELECT 1
                        FROM snippets_fts
                        WHERE  name    LIKE '%' || ?1 || '%'
                            OR content LIKE '%' || ?1 || '%'
                    )
                UNION
                    SELECT word
                    FROM snippets_spellfix
                    WHERE word MATCH ?1 AND top = 1
            ", (term,), |row| row.get(0))?;
            corrected_query.push(format!("%{spell_fixed}%"));
        }

        Ok(corrected_query.join(" "))
    }
}
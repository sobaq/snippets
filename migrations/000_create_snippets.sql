CREATE         TABLE snippets (name TEXT, content TEXT, created_at REAL DEFAULT (unixepoch('subsecond')));
CREATE VIRTUAL TABLE snippets_fts      USING fts5(name, content, content=snippets);
CREATE VIRTUAL TABLE snippets_terms    USING fts5vocab(snippets_fts, instance);
CREATE VIRTUAL TABLE snippets_spellfix USING spellfix1;

CREATE TRIGGER snippets_before_delete
    BEFORE DELETE ON snippets
BEGIN
    DELETE FROM snippets_spellfix
        WHERE word IN (SELECT term FROM snippets_terms WHERE doc = old.rowid);

    INSERT INTO snippets_fts (snippets_fts, rowid, name, content)
        VALUES ('delete', old.rowid, old.name, old.content);
END;

CREATE TRIGGER snippets_after_insert
    AFTER INSERT ON snippets
BEGIN
    INSERT INTO snippets_fts (rowid, name, content)
        VALUES (new.rowid, new.name, new.content);

    INSERT INTO snippets_spellfix (word)
        SELECT term FROM snippets_terms
            WHERE doc = new.rowid;
END;

CREATE TRIGGER snippets_after_update
    AFTER UPDATE ON snippets
BEGIN
    DELETE FROM snippets_spellfix
        WHERE word IN (SELECT term FROM snippets_terms WHERE doc = old.rowid);

    INSERT INTO snippets_fts (snippets_fts, rowid, name, content)
        VALUES ('delete', old.rowid, old.name, old.content);
    
    INSERT INTO snippets_fts (rowid, name, content)
        VALUES (new.rowid, new.name, new.content);

    INSERT INTO snippets_spellfix (word)
        SELECT term FROM snippets_terms
            WHERE doc = new.rowid;
END;

INSERT INTO snippets (name, content) VALUES ('Recent serious system logs', 'doas journalctl -p0..3 -rx');
INSERT INTO snippets (name, content) VALUES ('Strip video metadata', 'ffmpeg -i $IN -map_metadata -1 -c:v copy -c:a copy $OUT');
INSERT INTO snippets (name, content) VALUES ('Scale down video', 'ffmpeg -i $IN -s 720x480 -c:a copy $OUT');
INSERT INTO snippets (name, content) VALUES ('Generate 50 random 6 character strings',
    'for i in $(seq 50); do cat /dev/urandom | tr -dc a-z | head -c6; printf ''\\n''; done');
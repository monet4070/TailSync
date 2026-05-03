import sqlite3
import os
from tailsync.constants import DB_PATH, BASE_DIR

class HistoryDB:
    def __init__(self):
        if not os.path.exists(BASE_DIR): os.makedirs(BASE_DIR)
        self._init_db()

    def _init_db(self):
        with sqlite3.connect(DB_PATH) as conn:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    time TEXT, type TEXT, desc TEXT, data TEXT
                )
            """)

    def add(self, t_str, t_type, desc, data):
        with sqlite3.connect(DB_PATH) as conn:
            conn.execute("INSERT INTO history (time, type, desc, data) VALUES (?, ?, ?, ?)",
                         (t_str, t_type, desc, data))

    def get_all(self, keyword=None):
        query = "SELECT time, type, desc, data FROM history"
        params = []
        if keyword:
            query += " WHERE desc LIKE ? OR data LIKE ?"
            params = [f"%{keyword}%", f"%{keyword}%"]
        query += " ORDER BY id DESC"
        with sqlite3.connect(DB_PATH) as conn:
            return [{"time": r[0], "type": r[1], "desc": r[2], "data": r[3]} for r in conn.execute(query, params).fetchall()]

    def trim(self, limit):
        with sqlite3.connect(DB_PATH) as conn:
            to_delete = conn.execute(f"SELECT data FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT {limit}) AND type IN ('image', 'file')").fetchall()
            for (path,) in to_delete:
                if os.path.exists(path) and path.startswith(BASE_DIR):
                    try: os.remove(path)
                    except: pass
            conn.execute(f"DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY id DESC LIMIT {limit})")
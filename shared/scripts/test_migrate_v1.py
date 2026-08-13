import base64
import sqlite3
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import migrate_v1  # noqa: E402


class MigrationTests(unittest.TestCase):
    def test_reads_real_legacy_sqlite_rows_in_id_order(self):
        with tempfile.TemporaryDirectory() as directory:
            database = Path(directory) / "history.db"
            connection = sqlite3.connect(database)
            connection.execute(
                "CREATE TABLE history (id INTEGER, time TEXT, type TEXT, desc TEXT, data BLOB)"
            )
            connection.execute(
                "INSERT INTO history VALUES (2, 'later', 'file', 'report', X'02')"
            )
            connection.execute(
                "INSERT INTO history VALUES (1, 'earlier', 'text', 'secret', X'01')"
            )
            connection.commit()
            connection.close()

            rows = migrate_v1.read_legacy_rows(database)
            self.assertEqual([row[0] for row in rows], [1, 2])

    def test_masks_text_description_and_streams_bounded_chunks(self):
        plaintext = b"x" * (migrate_v1.IMPORT_CHUNK_SIZE + 7)
        requests = []

        def api_request(payload):
            requests.append(payload)
            if payload["cmd"] == "begin_import":
                return {"ok": True, "data": {"import_id": "import-1"}}
            return {"ok": True}

        migrated, skipped = migrate_v1.migrate_rows(
            [(1, "2026-01-01T00:00:00Z", "text", "plaintext password", b"ciphertext")],
            lambda _token: plaintext,
            "a" * 64,
            api_request,
            lambda _message: None,
        )

        self.assertEqual((migrated, skipped), (1, 0))
        self.assertEqual(requests[0]["desc"], migrate_v1.TEXT_DESCRIPTION_PLACEHOLDER)
        chunks = [request for request in requests if request["cmd"] == "import_chunk"]
        self.assertEqual([request["import_offset"] for request in chunks], [0, migrate_v1.IMPORT_CHUNK_SIZE])
        reconstructed = b"".join(base64.b64decode(request["chunk_b64"]) for request in chunks)
        self.assertEqual(reconstructed, plaintext)

    def test_bad_ciphertext_is_reported_and_skipped(self):
        messages = []
        migrated, skipped = migrate_v1.migrate_rows(
            [(7, "now", "text", "secret", b"bad")],
            lambda _token: (_ for _ in ()).throw(ValueError("invalid token")),
            "b" * 64,
            lambda _payload: self.fail("API should not be called"),
            messages.append,
        )
        self.assertEqual((migrated, skipped), (0, 1))
        self.assertIn("skip id=7", messages[0])


if __name__ == "__main__":
    unittest.main()

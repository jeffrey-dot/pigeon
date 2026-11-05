#!/usr/bin/env python3
"""Real CLI / PTY checks against core's safe API; no duplicate OSC parser.

Requires Python 3.8+, the Rust toolchain and cached Cargo dependencies.
Runs in an isolated HOME without reading shell rc files or requiring a GUI.
"""

import base64
import errno
import json
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
REPORT = ROOT / "tools/report/pigeon-report"
WRAPPER = ROOT / "tools/report/pigeon-run"


class ReportChecks(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="pigeon-report-check-")
        cls.addClassCleanup(cls.temporary.cleanup)
        cls.work = Path(cls.temporary.name)
        cls.env = {
            "HOME": str(cls.work), "XDG_CONFIG_HOME": str(cls.work / "config"),
            "PATH": str(Path(sys.executable).parent) + os.pathsep + os.defpath,
            "TERM": "xterm-256color", "PI_PROGRAM_STATUS": "1",
            "LC_ALL": "C", "PYTHONUTF8": "1",
        }
        manifest = cls.work / "Cargo.toml"
        manifest.write_text(f'''[package]
name = "pigeon-report-probe"
version = "0.0.0"
edition = "2021"
license = "GPL-3.0-or-later"
[dependencies]
pigeon-core = {{ path = {json.dumps(str(ROOT / 'core'))}, default-features = false }}
[[bin]]
name = "pigeon-report-probe"
path = {json.dumps(str(ROOT / 'tools/checks/helpers/report-probe.rs'))}
[lints.rust]
unsafe_code = "deny"
[lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
''')
        cargo = ["--offline", "--quiet", "--manifest-path", str(manifest),
                 "--target-dir", str(ROOT / "core/target")]
        subprocess.run(["cargo", "build", *cargo], check=True, cwd=ROOT)
        subprocess.run(["cargo", "clippy", *cargo, "--", "-D", "warnings"], check=True, cwd=ROOT)
        cls.probe = ROOT / "core/target/debug/pigeon-report-probe"

    def cli(self, *args, env=None):
        return subprocess.run([sys.executable, str(REPORT), *args],
                              capture_output=True, env=env or self.env,
                              start_new_session=True, cwd=self.work, timeout=10)

    def emit(self, *args):
        result = self.cli(*args, "--stdout")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, b"")
        self.assertTrue(result.stdout.startswith(b"\x1b]7501;"))
        self.assertTrue(result.stdout.endswith(b"\x1b\\"))
        return result.stdout

    def records(self, data):
        result = subprocess.run([str(self.probe)], input=data, capture_output=True,
                                check=True, env=self.env, cwd=self.work, timeout=10)
        return [tuple(line.split("\t")) for line in result.stdout.decode("utf-8").splitlines()]

    def terminal(self, command, trigger=None, env=None):
        pid, master = pty.fork()
        if pid == 0:
            os.chdir(self.work)
            os.execve(command[0], command, env or self.env)
        output = bytearray()
        triggered, finished, status = False, False, None
        deadline = time.monotonic() + 10
        try:
            while time.monotonic() < deadline:
                readable, _, _ = select.select([master], [], [], 0.05)
                if readable:
                    try:
                        chunk = os.read(master, 65536)
                    except OSError as error:
                        if error.errno != errno.EIO:
                            raise
                        chunk = b""
                    if not chunk:
                        break
                    output.extend(chunk)
                if trigger is not None and not triggered and b"READY" in output:
                    trigger(pid, master)
                    triggered = True
            else:
                self.fail("PTY command timed out")
            _, status = os.waitpid(pid, 0)
            finished = True
            if trigger is not None:
                self.assertTrue(triggered, output)
        finally:
            if not finished:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                os.waitpid(pid, 0)
            os.close(master)
        code = os.WEXITSTATUS(status) if os.WIFEXITED(status) else -os.WTERMSIG(status)
        return code, bytes(output)

    def test_states_and_blocked_kinds(self):
        for state in ("idle", "working", "blocked", "done", "error"):
            with self.subTest(state=state):
                self.assertEqual(self.records(self.emit(state)), [("", state, "", "", "")])
        for kind in ("permission", "question", "auth"):
            self.assertEqual(self.records(self.emit("blocked", "--kind", kind)),
                             [("", "blocked", kind, "", "")])

    def test_unicode_and_delimiters_roundtrip(self):
        message = "确认: a=b; 'quotes' / + e\u0301 👩\u200d💻"
        self.assertEqual(self.records(self.emit("blocked", "--id", "shell_1.agent-2",
                                              "--kind", "question", "--app", "工具 = one",
                                              "--msg", message)),
                         [("shell_1.agent-2", "blocked", "question", "工具 = one", message)])
        self.assertEqual(self.records(self.emit("working", "--msg", "", "--app", "")),
                         [("", "working", "", "", "")])

    def test_invalid_arguments_emit_nothing(self):
        cases = [["future"], ["working", "--kind", "permission"],
                 ["blocked", "--kind", "future"], ["done", "--all"],
                 ["clear"], ["clear", "--id", "a", "--all"],
                 ["clear", "--all", "--msg", ""], ["working", "--app", "a:state=done"]]
        for record_id in ("", ".a", "a.", "a..b", "a/b", "a:b", "a=b", "a b", "中文"):
            cases.append(["working", "--id", record_id])
        for field in ("--msg", "--app", "--id"):
            for control in ("\x01", "\x07", "\x1b", "\n", "\t", "\x7f", "\x85", "\x9f"):
                cases.append(["working", field, "x" + control + "y"])
            cases.append(["working", field, "x" * 4097])
        cases.append(["working", "--msg", "\udcff"])
        for args in cases:
            with self.subTest(args=repr(args)[:100]):
                result = self.cli(*args, "--stdout")
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, b"")
                self.assertNotIn(b"\x1b", result.stderr)

    def test_raw_and_encoded_lengths_include_framing(self):
        # This is a sender boundary, with acceptance checked by the real receiver.
        payload = self.emit("done", "--app", "abc", "--msg", "x" * 3048)
        self.assertEqual(len(payload), 4096)
        self.assertEqual(self.records(payload)[0][-1], "x" * 3048)
        for args, reason in [
            (["done", "--app", "abc", "--msg", "x" * 3049], b"Encoded report"),
            (["done", "--msg", "界" * 1100], b"Encoded report"),
            (["done", "--msg", "x" * 4080], b"Raw report"),
            (["done", "--app", "x" * 2100, "--id", "y" * 2100], b"Raw report"),
        ]:
            result = self.cli(*args, "--stdout")
            self.assertEqual(result.returncode, 2)
            self.assertIn(reason, result.stderr)
            self.assertEqual(result.stdout, b"")

    def test_clear_tree_and_prompt_lifecycle(self):
        data = b"".join(self.emit("done", "--id", record_id)
                        for record_id in ("a", "a.1", "a.1.x", "ab"))
        self.assertEqual(self.records(data + self.emit("clear", "--id", "a")),
                         [("ab", "done", "", "", "")])
        self.assertEqual(self.records(data + self.emit("clear", "--all")), [])
        data = b"".join(self.emit(state, "--id", state)
                        for state in ("idle", "working", "blocked", "done", "error"))
        self.assertEqual([row[1] for row in self.records(data + b"\x1b]133;A;cl=m\x1b\\")],
                         ["done", "error"])

    def test_core_rejects_corrupted_reports_and_ignores_unknown_fields(self):
        valid = self.emit("done", "--id", "r", "--msg", "ok")
        for bad in (b"***", base64.b64encode(b"bad\x00text"), base64.b64encode(b"\xff")):
            self.assertEqual(self.records(valid.replace(b"b2s=", bad)), [])
        self.assertEqual(self.records(valid.replace(b"id=r", b"id=r\x01")), [])
        self.assertEqual(self.records(valid[:-2] + b":future=" + b"x" * 4096 + valid[-2:]), [])
        extra = valid[:-2] + b":future=value:revision=999" + valid[-2:]
        self.assertEqual(self.records(extra), self.records(valid))
        self.assertEqual(self.records(valid.replace(b"state=done", b"state=?")), [])

    def test_no_tty_fails_even_with_capability_hint(self):
        result = self.cli("working")
        self.assertEqual(result.returncode, 2)
        self.assertIn(b"No writable controlling TTY", result.stderr)
        self.assertEqual(result.stdout, b"")

    def test_dry_run_is_redacted_and_does_not_claim_monitoring(self):
        secret = "not-a-real-secret"
        result = self.cli("blocked", "--id", secret, "--app", secret, "--msg", secret, "--dry-run")
        self.assertEqual(result.returncode, 0)
        self.assertNotIn(secret.encode(), result.stdout + result.stderr)
        info = json.loads(result.stdout)
        self.assertFalse(info["tty_available"])
        self.assertFalse(info["emitted"])
        self.assertFalse(info["receiver_verified"])
        self.assertTrue(info["capability_hint"])

    def test_tty_output_bypasses_redirected_stdout(self):
        target = self.work / "redirected"
        command = ["/bin/sh", "-c", 'exec "$1" working > "$2"', "sh", str(REPORT), str(target)]
        code, output = self.terminal(command)
        self.assertEqual(code, 0, output)
        self.assertEqual(target.read_bytes(), b"")
        self.assertEqual(self.records(output), [("", "working", "", "", "")])

    def test_tty_without_hint_warns_and_explicit_stdout_works(self):
        env = dict(self.env)
        del env["PI_PROGRAM_STATUS"]
        code, output = self.terminal([str(REPORT), "working"], env=env)
        self.assertEqual(code, 0)
        self.assertIn(b"support is unconfirmed", output)
        self.assertEqual(self.records(output), [("", "working", "", "", "")])
        self.assertEqual(self.cli("working", "--stdout", env=env).returncode, 0)

    def test_wrapper_preserves_status_and_arguments(self):
        for code in (0, 7, 130, 143):
            result, output = self.terminal([str(WRAPPER), "run", "Tests", "--", "/bin/sh", "-c",
                                            'test "$1" = "a b; c" || exit 99; exit "$2"',
                                            "sh", "a b; c", str(code)])
            self.assertEqual(result, code, output)
            records = self.records(output)
            self.assertEqual(records[0][1], "done" if code == 0 else "error")
        result, output = self.terminal([str(WRAPPER), "run", "Tests", "--", "/nonexistent/pigeon-command"])
        self.assertEqual(result, 127)
        self.assertEqual(self.records(output)[0][1], "error")

    def test_wrapper_preserves_option_like_fields(self):
        code, output = self.terminal([str(WRAPPER), "-run", "--help", "--", "/bin/true"])
        self.assertEqual(code, 0, output)
        self.assertEqual(self.records(output)[0][:4], ("-run", "done", "", "--help"))

    def test_wrapper_builtins_cannot_replace_or_untrap_the_wrapper(self):
        for command in [["exec", "/bin/true"], ["trap", "", "EXIT"]]:
            code, output = self.terminal([str(WRAPPER), "run", "Tests", "--", *command])
            self.assertEqual(code, 0, output)
            self.assertEqual(self.records(output)[0][1], "done")

    def test_wrapper_rejects_insufficient_terminal_report_budget_before_launch(self):
        code, output = self.terminal([str(WRAPPER), "r" * 4036, "", "--", "/bin/sh", "-c", "printf STARTED; exit 7"])
        self.assertEqual(code, 2, output)
        self.assertNotIn(b"STARTED", output)
        self.assertEqual(self.records(output), [])

    @unittest.skipUnless(Path("/dev/full").exists(), "requires full device")
    def test_dry_run_write_failure_uses_documented_exit_code(self):
        with open("/dev/full", "wb") as output:
            result = subprocess.run([sys.executable, str(REPORT), "working", "--dry-run"],
                                    stdout=output, stderr=subprocess.PIPE, env=self.env, timeout=10)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertNotIn(b"Exception ignored", result.stderr)

    def test_wrapper_keeps_stdin_and_nested_ids(self):
        script = ('"$1" "${PIGEON_REPORT_ID}.child" Child -- /bin/sh -c "exit 7"; '
                  '"$2" working --id "$PIGEON_REPORT_ID"; printf READY; read line; test "$line" = ok')
        code, output = self.terminal([str(WRAPPER), "parent", "Tests", "--", "/bin/sh", "-c",
                                     script, "sh", str(WRAPPER), str(REPORT)],
                                    trigger=lambda pid, master: os.write(master, b"ok\n"))
        self.assertEqual(code, 0, output)
        self.assertEqual([(row[0], row[1]) for row in self.records(output)],
                         [("parent", "done"), ("parent.child", "error")])

    def test_wrapper_signals_never_report_done(self):
        for sig in (signal.SIGHUP, signal.SIGINT, signal.SIGTERM):
            code, output = self.terminal([str(WRAPPER), "run", "Tests", "--", sys.executable,
                                         "-c", "import time; print('READY', flush=True); time.sleep(30)"],
                                        trigger=lambda pid, master, sig=sig: os.killpg(pid, sig))
            self.assertEqual(code, 128 + sig, output)
            self.assertEqual(self.records(output)[0][1], "error")

    def test_wrapper_traps_signal_even_if_child_exits_zero(self):
        child = "import signal,time; signal.signal(signal.SIGTERM, lambda *_: exit(0)); print('READY',flush=True); time.sleep(30)"
        code, output = self.terminal([str(WRAPPER), "run", "Tests", "--", sys.executable, "-c", child],
                                    trigger=lambda pid, master: os.killpg(pid, signal.SIGTERM))
        self.assertEqual(code, 143, output)
        self.assertEqual(self.records(output)[0][1], "error")

    def test_wrapper_kill_has_no_false_terminal_record(self):
        code, output = self.terminal([str(WRAPPER), "run", "Tests", "--", sys.executable,
                                     "-c", "import time; print('READY', flush=True); time.sleep(30)"],
                                    trigger=lambda pid, master: os.killpg(pid, signal.SIGKILL))
        self.assertEqual(code, -signal.SIGKILL, output)
        self.assertEqual(self.records(output)[0][1], "working")
        self.assertEqual(self.records(output + b"\x1b]133;A\x1b\\"), [])

    def test_installed_wrapper_preserves_exit_if_final_report_fails(self):
        installed = self.work / "bin"
        installed.mkdir()
        for source in (REPORT, WRAPPER):
            shutil.copy2(source, installed / source.name)
        code, output = self.terminal([str(installed / WRAPPER.name), "run", "Tests", "--",
                                     "/bin/sh", "-c", 'rm "$1"; exit 9', "sh",
                                     str(installed / REPORT.name)])
        self.assertEqual(code, 9, output)
        self.assertEqual(self.records(output)[0][1], "working")

    def test_wrapper_missing_tty_does_not_start_command(self):
        marker = self.work / "should-not-exist"
        result = subprocess.run([str(WRAPPER), "run", "Tests", "--", "/bin/sh", "-c",
                                 'touch "$1"', "sh", str(marker)], capture_output=True,
                                env=self.env, start_new_session=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(marker.exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)

"""Exercise real Tauri windows and settings IPC through native accessibility/input."""
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import subprocess
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi
from PIL import ImageGrab

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "src-tauri/target"
WORKSPACE = TARGET / "native-workspace"


def until(operation, timeout=15):
    end = time.monotonic() + timeout
    last = None
    while time.monotonic() < end:
        try:
            result = operation()
            if result:
                return result
        except Exception as error:
            last = error
        time.sleep(0.15)
    raise AssertionError(f"Native condition did not become true: {last}")


class NativeInput:
    def __init__(self):
        self.x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
        self.xtst = ctypes.CDLL(ctypes.util.find_library("Xtst"))
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.display = self.x11.XOpenDisplay(None)
        assert self.display, "X11 display is required for the native test"
        self.x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
        self.x11.XStringToKeysym.restype = ctypes.c_ulong
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.restype = ctypes.c_uint
        self.x11.XFlush.argtypes = [ctypes.c_void_p]
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        self.x11.XDefaultRootWindow.restype = ctypes.c_ulong
        self.x11.XQueryTree.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)), ctypes.POINTER(ctypes.c_uint)]
        self.x11.XFetchName.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_char_p)]
        self.x11.XFree.argtypes = [ctypes.c_void_p]
        self.x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
        self.x11.XRaiseWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XResizeWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_uint, ctypes.c_uint]
        self.xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]

    def key(self, name, down):
        symbol = self.x11.XStringToKeysym(name.encode("ascii"))
        code = self.x11.XKeysymToKeycode(self.display, symbol)
        assert code, f"No native key code for {name}"
        self.xtst.XTestFakeKeyEvent(self.display, code, int(down), 0)
        self.x11.XFlush(self.display)

    def press(self, name, control=False):
        if control:
            self.key("Control_L", True)
        self.key(name, True)
        self.key(name, False)
        if control:
            self.key("Control_L", False)
        time.sleep(0.1)

    def windows(self):
        if getattr(self, "app", None):
            return [(index, node.get_name()) for index,node in enumerate(descendants(self.app)) if node.get_role_name() == "frame"]
        root = self.x11.XDefaultRootWindow(self.display)
        pending = [(root, 0)]
        result = []
        while pending:
            window, depth = pending.pop()
            name = ctypes.c_char_p()
            if self.x11.XFetchName(self.display, window, ctypes.byref(name)) and name.value:
                text = name.value.decode("utf-8", errors="replace")
                if text.startswith("Hissabeth"):
                    result.append((window, text))
                self.x11.XFree(name)
            if depth >= 3:
                continue
            root_return = ctypes.c_ulong()
            parent_return = ctypes.c_ulong()
            child_pointer = ctypes.POINTER(ctypes.c_ulong)()
            count = ctypes.c_uint()
            if self.x11.XQueryTree(self.display, window, ctypes.byref(root_return), ctypes.byref(parent_return), ctypes.byref(child_pointer), ctypes.byref(count)):
                pending.extend((child_pointer[index], depth + 1) for index in range(count.value))
                if child_pointer:
                    self.x11.XFree(child_pointer)
        return result

    def focus(self, title):
        if getattr(self, "app", None):
            frames = [node for node in descendants(self.app) if node.get_role_name() == "frame" and title in node.get_name()]
            assert len(frames) == 1, [(n.get_name(), n.get_role_name()) for n in frames]
            assert frames[0].get_component_iface().grab_focus()
            time.sleep(0.15)
            return
        matches = [(window, name) for window, name in self.windows() if title in name]
        assert len(matches) == 1, matches
        window, _ = matches[0]
        self.x11.XRaiseWindow(self.display, window)
        self.x11.XSetInputFocus(self.display, window, 2, 0)
        self.x11.XFlush(self.display)
        time.sleep(0.15)

    def resize(self, title, width, height):
        matches = [(window, name) for window, name in self.windows() if title in name]
        assert len(matches) == 1, matches
        self.x11.XResizeWindow(self.display, matches[0][0], width, height)
        self.x11.XFlush(self.display)
        time.sleep(0.4)

    def close(self):
        self.x11.XCloseDisplay(self.display)


def children(node):
    return [node.get_child_at_index(index) for index in range(node.get_child_count())]


def descendants(node, maximum=5000):
    pending = [node]
    seen = 0
    while pending:
        current = pending.pop(0)
        if current is None:
            continue
        yield current
        seen += 1
        assert seen <= maximum, "Native accessibility tree exceeded the inspection limit"
        try:
            pending.extend(children(current))
        except Exception:
            pass


def app_for_pid(pid):
    desktop = Atspi.get_desktop(0)
    for child in children(desktop):
        if child and child.get_process_id() == pid:
            return child
    return None


def find(app, name, role=None):
    for node in descendants(app):
        if node.get_name() == name and (role is None or node.get_role_name() == role):
            return node
    return None


def activate(node):
    assert node is not None
    action = node.get_action_iface()
    assert action and action.get_n_actions(), f"No native action for {node.get_name()}"
    assert action.do_action(0), f"Native action failed for {node.get_name()}"


def save_tree(app, filename):
    rows = []
    for node in descendants(app):
        rows.append({"name": node.get_name(), "role": node.get_role_name()})
    (TARGET / filename).write_text(json.dumps(rows, ensure_ascii=False, indent=2))


def read_preferences():
    return json.loads((WORKSPACE / "preferences.json").read_text())["snapshot"]


def select_with_keys(node, key, native):
    assert node, "Native selector was not found"
    assert node.get_component_iface().grab_focus()
    native.press(key)
    native.press("Return")


def launch():
    log = open(TARGET / "industrial-native-app.log", "ab")
    process = subprocess.Popen([str(TARGET / "debug/hissabeth")], cwd=ROOT,
        env={**os.environ, "HISSABETH_DATA_DIR":str(WORKSPACE),
             "NO_AT_BRIDGE":"0",
             "GTK_A11Y":"always", "GDK_BACKEND":"x11", "WEBKIT_DISABLE_COMPOSITING_MODE":"1"},
        stdout=log, stderr=log)
    return process, log


def stop(process, log):
    process.terminate()
    process.wait(timeout=15)
    log.close()


def exercise():
    # This existing build workspace belongs to the native fixtures, never the user's application data.
    prefs={"locale":"en-US","appearance":{"theme":"light","accent":"blue","density":"compact","fontSize":13,"reducedMotion":False},
        "workspace":{"defaultView":"Layout","restoreLastView":True,"showNavigator":True,"showPrescriptionDock":True,"showMetrics":True,"showStatusBar":True,"navigatorSize":18,"dockSize":35},
        "display":{"decimals":4,"layoutRayArrows":True,"plotGrid":True},
        "analysis":{"samplingKind":"grid","gridSize":33,"hexRings":8,"reportPrecision":6,"pollIntervalMs":1500},
        "agents":{"targets":[],"autoRegister":False}}
    (WORKSPACE / "preferences.json").write_text(json.dumps({"snapshot":{"preferences":prefs,"revision":0}}))
    fixture_dir = TARGET / "agent-cli-fixtures"
    commands_path = fixture_dir / "native-selected-commands.jsonl"
    commands_path.unlink(missing_ok=True)
    for agent in ("codex", "claude"):
        script = fixture_dir / agent
        script.write_text("#!/usr/bin/python3\nimport json,sys\nfrom pathlib import Path\nbase=Path(__file__).resolve().parent\nwith (base/'native-selected-commands.jsonl').open('a') as f:f.write(json.dumps({'name':Path(sys.argv[0]).name,'args':sys.argv[1:]})+'\\n')\nif sys.argv[2]=='list':print('[]');sys.exit(0)\nprint('Fixture registered')\n")
        script.chmod(0o755)
    os.environ["PATH"] = str(fixture_dir) + os.pathsep + os.environ["PATH"]
    os.environ.pop("HISSABETH_SKIP_AGENT_REGISTRATION",None)
    native=NativeInput();process,log=launch();app=None
    try:
        app=until(lambda:app_for_pid(process.pid),30)
        native.app=app
        until(lambda:find(app,"Layout"),30)
        activate(until(lambda:find(app,"Settings","button")))
        until(lambda:any("Settings" in name for _,name in native.windows()))
        until(lambda:find(app,"Appearance"),20)
        activate(find(app,"Appearance"))
        theme=until(lambda:find(app,"Theme"))
        select_with_keys(theme,"End",native)
        activate(find(app,"Apply","button"))
        until(lambda:read_preferences()["preferences"]["appearance"]["theme"]=="high-contrast")
        assert read_preferences()["revision"]==1
        # Only selected native agent commands reach the isolated CLI fixtures.
        activate(find(app,"Agents"))
        activate(until(lambda:find(app,"Codex","check box")))
        activate(find(app,"Connect selected agents","button"))
        until(lambda:commands_path.exists() and len(commands_path.read_text().splitlines())==2)
        commands=[json.loads(line) for line in commands_path.read_text().splitlines()]
        assert all(command["name"]=="codex" for command in commands),commands
        activate(find(app,"Register the selected agents automatically at startup","check box"))
        activate(find(app,"Apply","button"))
        until(lambda:read_preferences()["preferences"]["agents"]["autoRegister"])
        # The real main window receives the same event while the settings window remains open.
        activate(find(app,"Workspace"))
        checkbox=until(lambda:find(app,"Show navigator"))
        assert checkbox.get_state_set().contains(Atspi.StateType.CHECKED)
        activate(until(lambda:find(app,"Hide navigator","button")))
        until(lambda:not read_preferences()["preferences"]["workspace"]["showNavigator"])
        until(lambda:not find(app,"Show navigator").get_state_set().contains(Atspi.StateType.CHECKED))
        activate(until(lambda:find(app,"Settings","button")))
        until(lambda:any("Settings" in name for _,name in native.windows()))
        assert len([name for _,name in native.windows() if "Settings" in name])==1
        select_with_keys(until(lambda:find(app,"Language")),"Home",native)
        activate(find(app,"Apply","button"))
        until(lambda:read_preferences()["preferences"]["locale"]=="zh-CN")
        until(lambda:any("光学工作台" in name for _,name in native.windows()))
        until(lambda:find(app,"语言"))
        assert len([name for _,name in native.windows() if "设置" in name])==1
        try: ImageGrab.grab().save(TARGET / "industrial-native-settings.png")
        except OSError: print("Native screenshot unavailable from the WSL root drawable; browser screenshots retained",flush=True)
        activate(find(app,"关闭","button"))
        until(lambda:not any("设置" in name for _,name in native.windows()))
        print("PASS: real Tauri settings create/focus/single window; native theme and language Apply; disk revision; main-to-settings live update; native Close",flush=True)
        stop(process,log);process,log=launch()
        app=until(lambda:app_for_pid(process.pid),30)
        native.app=app
        until(lambda:any("光学工作台" in name for _,name in native.windows()))
        activate(until(lambda:find(app,"设置","button")))
        until(lambda:any("设置" in name for _,name in native.windows()))
        until(lambda:find(app,"语言"),30)
        until(lambda:len(commands_path.read_text().splitlines())==4)
        commands=[json.loads(line) for line in commands_path.read_text().splitlines()]
        assert all(command["name"]=="codex" for command in commands),commands
        assert read_preferences()["preferences"]["appearance"]["theme"]=="high-contrast"
        assert not read_preferences()["preferences"]["workspace"]["showNavigator"]
        print("PASS: native application restart retains locale, high-contrast theme and navigator preference; only selected Codex fixture registers at startup",flush=True)
    except Exception:
        if app:
            save_tree(app,"industrial-native-failure-tree.json")
        try: ImageGrab.grab().save(TARGET / "industrial-native-failure.png")
        except OSError: pass
        raise
    finally:
        if process.poll() is None:stop(process,log)
        native.close()


if __name__ == "__main__":
    exercise()

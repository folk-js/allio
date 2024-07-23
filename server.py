import http
from flask import Flask, jsonify, request
import Quartz
import threading
from flask_cors import CORS
# import os
# import time
# import pyautogui
# import pyperclip

app = Flask(__name__)
CORS(app)

pointer = {"x": 0, "y": 0}


def fetch_window_dims():
    window_list = Quartz.CGWindowListCopyWindowInfo(
        Quartz.kCGWindowListOptionOnScreenOnly
        | Quartz.kCGWindowListExcludeDesktopElements,
        Quartz.kCGNullWindowID,
    )
    visible_windows = []
    for window in window_list:
        id = window.get("kCGWindowNumber", 0)
        name = window.get("kCGWindowOwnerName", "Unknown")
        bounds = window.get("kCGWindowBounds", {})
        layer = window.get("kCGWindowLayer", 10)
        is_onscreen = window.get("kCGWindowIsOnscreen", False)
        is_not_tiny = bounds.get("Width", 0) > 1 and bounds.get("Height", 0) > 1

        if layer == 0 and is_onscreen and is_not_tiny:
            window_info = {
                "id": id,
                "name": name,
                "x": bounds.get("X", 0),
                "y": bounds.get("Y", 0),
                "w": bounds.get("Width", 0),
                "h": bounds.get("Height", 0),
            }
            visible_windows.append(window_info)
    return visible_windows


# def focus_plash():
#     script = """
#     tell application "Plash"
#         activate
#     end tell
#     """
#     os.system(f"osascript -e '{script}'")


def success():
    return jsonify({"status": "success", "message": "we did it"})


@app.route("/pointer", methods=["GET"])
def get_pointer():
    global pointer
    return jsonify(pointer)


@app.route("/pointer", methods=["POST"])
def set_pointer():
    data = request.json
    # raise (data)
    global pointer
    x = data.get("x")
    y = data.get("y")
    if not x or not y:
        raise ("missing mouse pos in json")

    pointer = {"x": x, "y": y}
    print(pointer)
    return success()


@app.route("/windows")
def windows():
    return jsonify(fetch_window_dims())


# @app.route("/write_text", methods=["POST"])
# def write_text():
#     data = request.json
#     window_id = data.get("window_id")
#     text = data.get("text")

#     # Focus the target window
#     focus_window(window_id)

#     # Simulate typing the text
#     pyautogui.write(text, interval=0.005)

#     focus_plash()

#     return success(window_id)


# @app.route("/paste_text", methods=["POST"])
# def paste_text():
#     data = request.json
#     window_id = data.get("window_id")
#     text = data.get("text")

#     pyperclip.copy(text)

#     # Focus the target window
#     focus_window(window_id)
#     # focus_window(window_id)

#     time.sleep(0.1)
#     with pyautogui.hold(["command"]):
#         time.sleep(0.1)
#         pyautogui.press("v")
#     # time.sleep(0.1)
#     # pyautogui.write("hello world")
#     # pyautogui.press("right")
#     # time.sleep(0.1)
#     # with pyautogui.hold(["command"]):
#     #     time.sleep(0.1)
#     #     pyautogui.press("v")
#     # pyautogui.press("right")

#     # focus_plash()

#     return success(window_id)


# def focus_window(window_id):
#     # Fetch the list of windows and find the one with the matching kCGWindowNumber
#     window_list = Quartz.CGWindowListCopyWindowInfo(
#         Quartz.kCGWindowListOptionOnScreenOnly
#         | Quartz.kCGWindowListExcludeDesktopElements,
#         Quartz.kCGNullWindowID,
#     )
#     target_window = next(
#         (window for window in window_list if window["kCGWindowNumber"] == window_id),
#         None,
#     )
#     if target_window:
#         pid = target_window["kCGWindowOwnerPID"]
#         script = f"""
#         tell application "System Events"
#             set frontmost of the first process whose unix id is {pid} to true
#         end tell
#         """
#         os.system(f"osascript -e '{script}'")


if __name__ == "__main__":
    threading.Thread(target=fetch_window_dims, daemon=True).start()
    app.run(debug=True)

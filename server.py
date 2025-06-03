from flask import Flask, jsonify
import Quartz
import threading
from flask_cors import CORS

app = Flask(__name__)
CORS(app)


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


def success():
    return jsonify({"status": "success", "message": "we did it"})


@app.route("/element", methods=["GET"])
def get_elements():
    global elements
    return jsonify(elements)


@app.route("/windows")
def windows():
    return jsonify(fetch_window_dims())


if __name__ == "__main__":
    threading.Thread(target=fetch_window_dims, daemon=True).start()
    app.run(debug=True)

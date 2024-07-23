import { register } from '@tauri-apps/api/globalShortcut';
import { getCurrent } from '@tauri-apps/api/window';
import { Tldraw } from 'tldraw'
import "./App.css";

const appWindow = getCurrent();
let clickthrough = false;
await register('CommandOrControl+Shift+E', () => {
  console.log('Toggling clickthrough');
  clickthrough = !clickthrough;
  appWindow.setIgnoreCursorEvents(clickthrough);
  // if (clickthrough) {
  //   appWindow.setFocus();
  // }
});

window.addEventListener('mousemove', (ev) => {
  const x = ev.screenX
  const y = ev.screenY

  fetch("http://127.0.0.1:5000/pointer", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ x, y })
  })

  // we can get pointer position...
  // const result = fetch("http://127.0.0.1:5000/pointer").then(r => console.log(r.json()))

})

export default function App() {
  return (
    <div style={{ position: 'fixed', inset: 0 }}>
      <Tldraw
        persistenceKey='overlay'
        components={{
          MenuPanel: null,
          DebugPanel: null,
          Minimap: null,
          ZoomMenu: null,
          HelpMenu: null
        }}
        cameraOptions={{
          isLocked: true
        }}

      />
    </div>
  )
}
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
  if (!clickthrough) {
    appWindow.setFocus();
  }
});

export default function App() {
  return (
    <div style={{ position: 'fixed', inset: 0 }}>
      <Tldraw
        persistenceKey='overlay'
        components={{
          MenuPanel: null,
          DebugPanel: null
        }}
        cameraOptions={{
          isLocked: true
        }}

      />
    </div>
  )
}
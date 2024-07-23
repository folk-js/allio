// given position from server (+ events in future)
// get element at that position
// make it red

(() => {
  let e = null
  async function getElementUnderCursor() {
  const response = await fetch("http://127.0.0.1:5000/pointer")
  const cursor = await response.json()
  // const offset = {x: window.screenX, y: window.screenY}
  console.log (cursor.y, window.screenTop)
  const MAGIC_OFFSET_LOL = 38 * 2
  if (e) {
    e.setAttribute("style", "")
  }

  e = document.elementFromPoint(cursor.x - window.screenX, cursor.y - window.screenTop - MAGIC_OFFSET_LOL)

  if (e) {
    e.setAttribute("style", "outline: 5px solid blue;")

    // console.log(e)
  }

  setTimeout(getElementUnderCursor, 100)
  }
  getElementUnderCursor()
})()
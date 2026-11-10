import { memo, useCallback, useReducer } from "react";
import { createRoot } from "react-dom/client";

const adjectives = ["pretty", "large", "big", "small", "tall", "short", "long", "handsome", "plain", "quaint", "clean", "elegant", "easy", "angry", "crazy", "helpful", "mushy", "odd", "unsightly", "adorable", "important", "inexpensive", "cheap", "expensive", "fancy"];
const colours = ["red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black", "orange"];
const nouns = ["table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger", "pizza", "mouse", "keyboard"];

function labelOf(id) {
  const a = (id * 16807) % 2147483647;
  const b = (a * 16807) % 2147483647;
  const c = (b * 16807) % 2147483647;
  return `${adjectives[a % adjectives.length]} ${colours[b % colours.length]} ${nouns[c % nouns.length]}`;
}

function made(from, n) {
  const out = new Array(n);
  for (let i = 0; i < n; i++) out[i] = { id: from + i, label: labelOf(from + i) };
  return out;
}

function reduce(state, action) {
  const { rows, nextId } = state;
  switch (action.type) {
    case "run":
      return { ...state, rows: made(nextId, 1000), nextId: nextId + 1000 };
    case "runlots":
      return { ...state, rows: made(nextId, 10000), nextId: nextId + 10000 };
    case "add":
      return { ...state, rows: rows.concat(made(nextId, 1000)), nextId: nextId + 1000 };
    case "update": {
      const next = rows.slice();
      for (let i = 0; i < next.length; i += 10) next[i] = { ...next[i], label: `${next[i].label} !!!` };
      return { ...state, rows: next };
    }
    case "clear":
      return { ...state, rows: [] };
    case "swaprows": {
      if (rows.length < 999) return state;
      const next = rows.slice();
      const moved = next[1];
      next[1] = next[998];
      next[998] = moved;
      return { ...state, rows: next };
    }
    case "remove":
      return { ...state, rows: rows.filter((row) => row.id !== action.id) };
    case "select":
      return { ...state, selected: action.id };
    default:
      return state;
  }
}

const Row = memo(function Row({ id, label, danger, dispatch }) {
  return (
    <tr className={danger ? "danger" : ""}>
      <td className="id">{id}</td>
      <td className="label">
        <button type="button" className="select" onClick={() => dispatch({ type: "select", id })}>
          {label}
        </button>
      </td>
      <td className="rm">
        <button type="button" className="remove" aria-label="Remove" onClick={() => dispatch({ type: "remove", id })}>
          x
        </button>
      </td>
      <td className="pad"></td>
    </tr>
  );
});

function App() {
  const [{ rows, selected }, dispatch] = useReducer(reduce, { rows: [], selected: 0, nextId: 1 });
  const on = useCallback((type) => () => dispatch({ type }), []);
  return (
    <main>
      <button id="run" type="button" onClick={on("run")}>Create 1,000 rows</button>
      <button id="runlots" type="button" onClick={on("runlots")}>Create 10,000 rows</button>
      <button id="add" type="button" onClick={on("add")}>Append 1,000 rows</button>
      <button id="update" type="button" onClick={on("update")}>Update every 10th row</button>
      <button id="clear" type="button" onClick={on("clear")}>Clear</button>
      <button id="swaprows" type="button" onClick={on("swaprows")}>Swap rows</button>
      <table>
        <tbody id="tbody">
          {rows.map((row) => (
            <Row key={row.id} id={row.id} label={row.label} danger={row.id === selected} dispatch={dispatch} />
          ))}
        </tbody>
      </table>
    </main>
  );
}

createRoot(document.getElementById("app")).render(<App />);

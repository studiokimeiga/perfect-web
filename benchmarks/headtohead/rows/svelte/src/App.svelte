<script>
  const adjectives = ["pretty", "large", "big", "small", "tall", "short", "long", "handsome", "plain", "quaint", "clean", "elegant", "easy", "angry", "crazy", "helpful", "mushy", "odd", "unsightly", "adorable", "important", "inexpensive", "cheap", "expensive", "fancy"];
  const colours = ["red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black", "orange"];
  const nouns = ["table", "chair", "house", "bbq", "desk", "car", "pony", "cookie", "sandwich", "burger", "pizza", "mouse", "keyboard"];

  function labelOf(id) {
    const a = (id * 16807) % 2147483647;
    const b = (a * 16807) % 2147483647;
    const c = (b * 16807) % 2147483647;
    return adjectives[a % adjectives.length] + " " + colours[b % colours.length] + " " + nouns[c % nouns.length];
  }

  class Row {
    id;
    label = $state("");
    constructor(id) {
      this.id = id;
      this.label = labelOf(id);
    }
  }

  let nextId = 1;
  let rows = $state.raw([]);
  let selected = $state(0);

  function made(n) {
    const out = new Array(n);
    for (let i = 0; i < n; i++) out[i] = new Row(nextId++);
    return out;
  }
  const run = () => (rows = made(1000));
  const runlots = () => (rows = made(10000));
  const add = () => (rows = [...rows, ...made(1000)]);
  const update = () => {
    for (let i = 0; i < rows.length; i += 10) rows[i].label += " !!!";
  };
  const clear = () => (rows = []);
  const swaprows = () => {
    if (rows.length < 999) return;
    const clone = rows.slice();
    const t = clone[1];
    clone[1] = clone[998];
    clone[998] = t;
    rows = clone;
  };
  const remove = (row) => {
    const clone = rows.slice();
    clone.splice(clone.indexOf(row), 1);
    rows = clone;
  };
</script>

<main>
  <button id="run" type="button" onclick={run}>Create 1,000 rows</button>
  <button id="runlots" type="button" onclick={runlots}>Create 10,000 rows</button>
  <button id="add" type="button" onclick={add}>Append 1,000 rows</button>
  <button id="update" type="button" onclick={update}>Update every 10th row</button>
  <button id="clear" type="button" onclick={clear}>Clear</button>
  <button id="swaprows" type="button" onclick={swaprows}>Swap rows</button>
  <table>
    <tbody id="tbody">
      {#each rows as row (row.id)}
        <tr class={selected === row.id ? "danger" : ""}>
          <td class="id">{row.id}</td>
          <td class="label"><button type="button" class="select" onclick={() => (selected = row.id)}>{row.label}</button></td>
          <td class="rm"><button type="button" class="remove" aria-label="Remove" onclick={() => remove(row)}>x</button></td>
          <td class="pad"></td>
        </tr>
      {/each}
    </tbody>
  </table>
</main>

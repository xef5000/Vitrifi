/* Plain browser JavaScript. The examples are explanatory snapshots, not a compiler. */
(() => {
  "use strict";

  const examples = window.VITRIFI_EXAMPLES;
  if (!Array.isArray(examples) || !examples.length) return;

  const byId = (id) => document.getElementById(id);
  const escape = (text) => text.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;"
  })[character]);

  function highlight(source) {
    const tokens = /\/\/[^\n]*|"(?:\\.|[^"\\])*"|@[A-Za-z][\w.]*|\b(?:public|private|return|throw|new|if|int|void|class|final)\b|\b[A-Z][A-Za-z0-9_]*\b|\b[a-z][A-Za-z0-9_]*(?=\()/g;
    let html = "";
    let previous = 0;
    for (const match of source.matchAll(tokens)) {
      const token = match[0];
      let kind = "keyword";
      if (token.startsWith("//")) kind = "comment";
      else if (token.startsWith('"')) kind = "string";
      else if (token.startsWith("@")) kind = "annotation";
      else if (/^[A-Z]/.test(token)) kind = "type";
      else if (!/^(public|private|return|throw|new|if|int|void|class|final)$/.test(token)) kind = "function";
      html += escape(source.slice(previous, match.index));
      html += `<span class="token-${kind}">${escape(token)}</span>`;
      previous = match.index + token.length;
    }
    return html + escape(source.slice(previous));
  }

  let selected = 0;
  let changed = false;
  const tablist = byId("scenario-tabs");
  const tabs = examples.map((example, index) => {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.id = `scenario-tab-${example.id}`;
    tab.setAttribute("role", "tab");
    tab.setAttribute("aria-controls", "scenario-panel");
    const number = document.createElement("span");
    number.className = "tab-index";
    number.textContent = String(index + 1).padStart(2, "0");
    number.setAttribute("aria-hidden", "true");
    tab.append(number, document.createTextNode(example.tab));
    tab.addEventListener("click", () => selectExample(index));
    tab.addEventListener("keydown", (event) => {
      let next;
      if (event.key === "ArrowRight") next = (index + 1) % examples.length;
      if (event.key === "ArrowLeft") next = (index - 1 + examples.length) % examples.length;
      if (event.key === "Home") next = 0;
      if (event.key === "End") next = examples.length - 1;
      if (next !== undefined) {
        event.preventDefault();
        selectExample(next);
        tabs[next].focus();
      }
    });
    tablist.append(tab);
    return tab;
  });

  function selectExample(index) {
    selected = index;
    changed = false;
    render();
  }

  function render() {
    const example = examples[selected];
    const version = changed ? example.after : example.before;
    tabs.forEach((tab, index) => {
      tab.setAttribute("aria-selected", String(index === selected));
      tab.tabIndex = index === selected ? 0 : -1;
    });
    byId("scenario-panel").setAttribute("aria-labelledby", tabs[selected].id);
    byId("scenario-label").textContent = example.label;
    byId("scenario-title").textContent = example.title;
    byId("scenario-description").textContent = example.description;
    byId("scenario-policy").textContent = example.policy;
    byId("before-button").setAttribute("aria-pressed", String(!changed));
    byId("after-button").setAttribute("aria-pressed", String(changed));
    byId("code-version").textContent = changed ? "CHANGED LINES HIGHLIGHTED" : "ORIGINAL";

    const originalLines = new Set(example.before.code.split("\n").map((line) => line.trim()));
    byId("scenario-code").innerHTML = version.code.split("\n").map((line, index) => {
      const added = changed && !originalLines.has(line.trim());
      return `<span class="code-line${added ? " line-added" : ""}"><span class="line-number" aria-hidden="true">${index + 1}</span><span>${highlight(line) || " "}</span></span>`;
    }).join("");

    byId("result-status").textContent = version.status.toUpperCase();
    document.querySelector(".demo-result").dataset.state = version.status;
    byId("result-label").textContent = version.label;
    byId("result-summary").textContent = version.summary;
    byId("result-command").textContent = version.command;
    byId("result-output").textContent = version.output;
    byId("scenario-explanation").textContent = example.explanation;
    byId("scenario-limit").textContent = example.limit;
    const path = byId("effect-path");
    path.replaceChildren();
    version.path.forEach((step, index) => {
      if (index) {
        const arrow = document.createElement("span");
        arrow.className = "path-arrow";
        arrow.textContent = "→";
        arrow.setAttribute("aria-hidden", "true");
        path.append(arrow);
      }
      const node = document.createElement("span");
      node.className = "path-node";
      node.textContent = step;
      path.append(node);
    });
  }

  byId("before-button").addEventListener("click", () => { changed = false; render(); });
  byId("after-button").addEventListener("click", () => { changed = true; render(); });

  const concepts = {
    feature: { title: "The capability, named.", description: "A feature groups source entry operations and the code they reach. Its name comes from @Feature in Java. The generated view gives that capability a place in the pull request.", origin: "Authored identity · derived relationships" },
    purpose: { title: "The reason it exists.", description: "A short explanation from your @Feature annotation. The compiler preserves this business intent. It does not turn prose into a verified rule; policy annotations provide the enforceable constraints.", origin: "Authored intent" },
    type: { title: "The vocabulary of the interface.", description: "A name used in an operation signature, mapped from Java types. Cart tells you what preview returns. This architectural view does not reproduce every field in the Java record.", origin: "Derived from Java types" },
    resource: { title: "Something the code can affect.", description: "Carts names the database resource. @ResourceTable maps the carts table to that name. Recognized SQL supplies the read and write facts; the annotation supplies their meaning.", origin: "Authored resource mapping" },
    operation: { title: "An entry point, made legible.", description: "The name, parameters, and return type come from Java. Its body connects implementation, effects, and calls. Here, export marks preview as a feature entry operation.", origin: "Derived from the compiler" },
    bind: { title: "A direct route to the source.", description: "The file path and native symbol identify the actual Java method, including its parameter types. Follow this link when the architectural view is not enough to understand a change.", origin: "Derived source identity" },
    effects: { title: "What this operation can affect.", description: "preview can read Carts through its repository call. Effects include recognized effects reachable through supported calls. Add a write in a helper and that write propagates back to its callers.", origin: "Derived and propagated effects" },
    calls: { title: "Follow the dependency.", description: "preview calls the resolved load operation. Find load in the full generated file to continue into the repository. Unknown dispatch is recorded as a gap and prevents a complete check.", origin: "Compiler + framework analysis" }
  };

  const conceptButtons = Array.from(document.querySelectorAll(".concept-buttons button"));
  function selectConcept(name) {
    const concept = concepts[name];
    conceptButtons.forEach((button) => button.setAttribute("aria-pressed", String(button.dataset.concept === name)));
    document.querySelectorAll("#generated-code .code-line").forEach((line) => line.classList.toggle("line-focus", line.dataset.concept === name));
    byId("concept-number").textContent = String(Object.keys(concepts).indexOf(name) + 1).padStart(2, "0");
    byId("concept-title").textContent = concept.title;
    byId("concept-description").textContent = concept.description;
    byId("concept-origin").textContent = concept.origin.toUpperCase();
  }
  conceptButtons.forEach((button) => button.addEventListener("click", () => selectConcept(button.dataset.concept)));

  byId("copy-commands").addEventListener("click", async () => {
    const commands = byId("quickstart-code").textContent;
    let copied = false;
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(commands);
        copied = true;
      }
    } catch { /* Local-file clipboard permissions vary by browser. */ }
    if (!copied) {
      const field = document.createElement("textarea");
      field.value = commands;
      field.setAttribute("aria-label", "Commands to copy");
      field.style.cssText = "position:fixed;left:-9999px;top:0";
      document.body.append(field);
      field.select();
      try { copied = document.execCommand("copy"); } catch { /* Offer manual selection below. */ }
      field.remove();
      byId("copy-commands").focus();
    }
    if (!copied) {
      const range = document.createRange();
      range.selectNodeContents(byId("quickstart-code"));
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    }
    byId("copy-status").textContent = copied ? "Copied to clipboard." : "Commands selected — copy with Ctrl/Cmd+C.";
  });

  render();
  selectConcept("feature");
  document.body.classList.add("js-ready");
})();

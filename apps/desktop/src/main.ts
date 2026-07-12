import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type Candidate = {
  text: string;
  reading: string[];
  score: number;
};

const PAGE_SIZE = 5;

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("App root was not found");
}

app.innerHTML = `
  <section class="shell">
    <header class="hero">
      <img class="brand-mark" src="/logo.svg" alt="NovaType" />
      <div>
        <p class="eyebrow">NovaType v0.1 练习场</p>
        <h1>候选窗预览（简化搜狗风格）</h1>
      </div>
    </header>

    <label class="output-panel">
      <span>已上屏文本（数字键 1–5 / 空格 / 点击候选上屏）</span>
      <textarea id="output" rows="3" spellcheck="false" placeholder="上屏的文字会出现在这里"></textarea>
    </label>

    <label class="input-panel">
      <span>拼音输入</span>
      <input id="query" autocomplete="off" spellcheck="false" placeholder="输入拼音，如 zhongguoren" />
    </label>

    <div id="ime-window" class="ime-window" hidden>
      <div id="composition" class="composition"></div>
      <div class="candidate-row">
        <ol id="candidates" class="candidates" aria-live="polite"></ol>
        <div class="paging">
          <button id="page-prev" type="button" aria-label="上一页">‹</button>
          <button id="page-next" type="button" aria-label="下一页">›</button>
        </div>
      </div>
    </div>

    <div id="predictions-panel" class="predictions-panel" hidden>
      <span class="predictions-label">联想</span>
      <div id="predictions" class="predictions"></div>
    </div>

    <p class="hint">翻页：<kbd>-</kbd> / <kbd>=</kbd>　清空拼音：<kbd>Esc</kbd>　该窗口是 v0.3 原生候选窗的设计稿预览</p>
  </section>
`;

function mustQuery<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`Practice UI failed to initialize: ${selector}`);
  }
  return element;
}

const queryInput = mustQuery<HTMLInputElement>("#query");
const outputArea = mustQuery<HTMLTextAreaElement>("#output");
const imeWindow = mustQuery<HTMLDivElement>("#ime-window");
const compositionLine = mustQuery<HTMLDivElement>("#composition");
const candidatesList = mustQuery<HTMLOListElement>("#candidates");
const pagePrev = mustQuery<HTMLButtonElement>("#page-prev");
const pageNext = mustQuery<HTMLButtonElement>("#page-next");
const predictionsPanel = mustQuery<HTMLDivElement>("#predictions-panel");
const predictionsRow = mustQuery<HTMLDivElement>("#predictions");

let allCandidates: Candidate[] = [];
let page = 0;

async function refreshCandidates(): Promise<void> {
  const input = queryInput.value.trim();

  if (!input) {
    allCandidates = [];
    page = 0;
    render();
    return;
  }

  allCandidates = await invoke<Candidate[]>("suggest", { input, limit: 20 });
  page = 0;
  render();
}

function render(): void {
  const input = queryInput.value.trim();

  if (!input || allCandidates.length === 0) {
    imeWindow.hidden = true;
    return;
  }

  imeWindow.hidden = false;
  compositionLine.textContent = formatComposition(input);

  const start = page * PAGE_SIZE;
  const pageItems = allCandidates.slice(start, start + PAGE_SIZE);
  candidatesList.replaceChildren(
    ...pageItems.map((candidate, index) => renderCandidate(candidate, index)),
  );

  pagePrev.disabled = page === 0;
  pageNext.disabled = start + PAGE_SIZE >= allCandidates.length;
}

function formatComposition(input: string): string {
  const best = allCandidates[0];
  if (best && best.reading.length > 0) {
    return best.reading.join("'");
  }
  return input;
}

function renderCandidate(candidate: Candidate, index: number): HTMLLIElement {
  const item = document.createElement("li");
  item.className = index === 0 && page === 0 ? "candidate selected" : "candidate";
  item.title = `score ${candidate.score.toFixed(2)}`;
  item.innerHTML = `
    <span class="candidate-index">${index + 1}.</span>
    <span class="candidate-text"></span>
  `;
  const textSlot = item.querySelector<HTMLSpanElement>(".candidate-text");
  if (textSlot) {
    textSlot.textContent = candidate.text;
  }
  item.addEventListener("click", () => {
    commit(candidate);
  });
  return item;
}

function commit(candidate: Candidate): void {
  void commitText(candidate.text, candidate.reading);
}

async function commitText(text: string, reading: string[]): Promise<void> {
  outputArea.value += text;
  queryInput.value = "";
  allCandidates = [];
  page = 0;
  render();
  queryInput.focus();

  try {
    const predictions = await invoke<string[]>("commit", { text, reading });
    renderPredictions(predictions);
  } catch (error) {
    console.error("commit failed", error);
    renderPredictions([]);
  }
}

function renderPredictions(predictions: string[]): void {
  if (predictions.length === 0) {
    predictionsPanel.hidden = true;
    predictionsRow.replaceChildren();
    return;
  }

  predictionsPanel.hidden = false;
  predictionsRow.replaceChildren(
    ...predictions.map((text) => {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "prediction-chip";
      chip.textContent = text;
      chip.addEventListener("click", () => {
        void commitText(text, []);
      });
      return chip;
    }),
  );
}

function commitByIndex(indexOnPage: number): void {
  const candidate = allCandidates[page * PAGE_SIZE + indexOnPage];
  if (candidate) {
    commit(candidate);
  }
}

function turnPage(delta: number): void {
  const lastPage = Math.max(0, Math.ceil(allCandidates.length / PAGE_SIZE) - 1);
  const next = Math.min(lastPage, Math.max(0, page + delta));
  if (next !== page) {
    page = next;
    render();
  }
}

queryInput.addEventListener("input", () => {
  predictionsPanel.hidden = true;
  void refreshCandidates();
});

queryInput.addEventListener("keydown", (event) => {
  if (allCandidates.length === 0) {
    return;
  }

  if (event.key >= "1" && event.key <= String(PAGE_SIZE)) {
    event.preventDefault();
    commitByIndex(Number(event.key) - 1);
    return;
  }

  switch (event.key) {
    case " ":
      event.preventDefault();
      commitByIndex(0);
      break;
    case "-":
      event.preventDefault();
      turnPage(-1);
      break;
    case "=":
      event.preventDefault();
      turnPage(1);
      break;
    case "Escape":
      event.preventDefault();
      queryInput.value = "";
      void refreshCandidates();
      break;
    default:
      break;
  }
});

pagePrev.addEventListener("click", () => {
  turnPage(-1);
});
pageNext.addEventListener("click", () => {
  turnPage(1);
});

queryInput.focus();
void refreshCandidates();
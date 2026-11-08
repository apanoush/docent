/**
 * Minimal Markdown rendering for the Firefox answer tab.
 *
 * This mirrors `plugins/qutebrowser/docent/markdown.py`. Only the constructs
 * that show up in model answers are supported, all text is escaped, and links
 * are limited to http, https, and mailto URLs.
 */

const HEADING = /^ {0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^ {0,3}([-*_])(?:\s*\1){2,}\s*$/;
const FENCE = /^ {0,3}```\s*([A-Za-z0-9_+-]*)\s*$/;
const UNORDERED = /^ {0,3}[-*+]\s+(.*)$/;
const ORDERED = /^ {0,3}\d{1,9}[.)]\s+(.*)$/;
const QUOTE = /^ {0,3}>\s?(.*)$/;
const SEPARATOR_CELL = /^:?-{3,}:?$/;

const INLINE =
  /`(?<code>[^`\n]+)`|!\[(?<imageAlt>[^\]\n]*)\]\((?<imageUrl>[^)\s]*)\)|\*\*(?<strong>.+?)\*\*|~~(?<strike>.+?)~~|(?<!\*)\*(?<em>[^*\n]+)\*(?!\*)|\[(?<label>[^\]\n]+)\]\((?<url>[^)\s]+)\)/g;

const SAFE_SCHEMES = new Set(["http", "https", "mailto"]);

/** Convert a Markdown subset to a safe HTML fragment. */
export function render(text) {
  if (!text.trim()) {
    return "";
  }
  const lines = text.replace(/\r\n/g, "\n").replace(/\r/g, "\n").split("\n");
  return blocks(lines).join("\n");
}

function blocks(lines) {
  const out = [];
  let paragraph = [];

  const flush = () => {
    if (paragraph.length) {
      out.push(`<p>${inline(paragraph.join(" "))}</p>`);
      paragraph = [];
    }
  };

  let index = 0;
  while (index < lines.length) {
    const line = lines[index];

    if (!line.trim()) {
      flush();
      index += 1;
      continue;
    }

    const fence = FENCE.exec(line);
    if (fence) {
      flush();
      const language = fence[1];
      index += 1;
      const code = [];
      while (index < lines.length && !FENCE.test(lines[index])) {
        code.push(lines[index]);
        index += 1;
      }
      index += 1;
      const attribute = language ? ` class="language-${language}"` : "";
      out.push(`<pre><code${attribute}>${escapeText(code.join("\n"))}</code></pre>`);
      continue;
    }

    const heading = HEADING.exec(line);
    if (heading) {
      flush();
      const level = heading[1].length;
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
      index += 1;
      continue;
    }

    if (RULE.test(line)) {
      flush();
      out.push("<hr>");
      index += 1;
      continue;
    }

    if (QUOTE.test(line)) {
      flush();
      const quoted = [];
      while (index < lines.length) {
        const nested = QUOTE.exec(lines[index]);
        if (!nested) {
          break;
        }
        quoted.push(nested[1]);
        index += 1;
      }
      out.push(`<blockquote>${render(quoted.join("\n"))}</blockquote>`);
      continue;
    }

    const unordered = UNORDERED.exec(line);
    const ordered = ORDERED.exec(line);
    if (unordered || ordered) {
      flush();
      const pattern = unordered ? UNORDERED : ORDERED;
      const tag = unordered ? "ul" : "ol";
      const items = [];
      while (index < lines.length) {
        const item = pattern.exec(lines[index]);
        if (!item) {
          break;
        }
        items.push(`<li>${inline(item[1])}</li>`);
        index += 1;
      }
      out.push(`<${tag}>${items.join("")}</${tag}>`);
      continue;
    }

    const table = renderTable(lines, index);
    if (table) {
      flush();
      out.push(table.html);
      index = table.index;
      continue;
    }

    paragraph.push(line.trim());
    index += 1;
  }

  flush();
  return out;
}

function renderTable(lines, start) {
  if (!lines[start].includes("|") || start + 1 >= lines.length) {
    return null;
  }
  const alignments = separatorAlignments(lines[start + 1]);
  if (!alignments) {
    return null;
  }
  const header = splitRow(lines[start]);
  if (header.length < 2 || header.length !== alignments.length) {
    return null;
  }

  const rows = [];
  let index = start + 2;
  while (index < lines.length && lines[index].trim() && lines[index].includes("|")) {
    let cells = splitRow(lines[index]);
    cells = cells.concat(Array(header.length).fill("")).slice(0, header.length);
    const rendered = cells
      .map((cell, column) => cellHtml("td", cell, alignments[column]))
      .join("");
    rows.push(`<tr>${rendered}</tr>`);
    index += 1;
  }

  const head = header
    .map((cell, column) => cellHtml("th", cell, alignments[column]))
    .join("");
  return {
    html:
      '<div class="table-wrap"><table>' +
      `<thead><tr>${head}</tr></thead>` +
      `<tbody>${rows.join("")}</tbody>` +
      "</table></div>",
    index,
  };
}

function separatorAlignments(line) {
  if (!line.includes("|")) {
    return null;
  }
  const cells = splitRow(line);
  if (cells.length < 2) {
    return null;
  }
  const alignments = [];
  for (const cell of cells) {
    if (!SEPARATOR_CELL.test(cell)) {
      return null;
    }
    if (cell.startsWith(":") && cell.endsWith(":")) {
      alignments.push("center");
    } else if (cell.endsWith(":")) {
      alignments.push("right");
    } else if (cell.startsWith(":")) {
      alignments.push("left");
    } else {
      alignments.push("");
    }
  }
  return alignments;
}

function splitRow(line) {
  let text = line.trim();
  if (text.startsWith("|")) {
    text = text.slice(1);
  }
  if (text.endsWith("|") && !text.endsWith("\\|")) {
    text = text.slice(0, -1);
  }
  return text.split(/(?<!\\)\|/).map((cell) => cell.trim().replace(/\\\|/g, "|"));
}

function cellHtml(tag, text, align) {
  const style = align ? ` style="text-align: ${align}"` : "";
  return `<${tag}${style}>${inline(text)}</${tag}>`;
}

function inline(text) {
  const out = [];
  let position = 0;
  for (const match of text.matchAll(INLINE)) {
    out.push(escapeText(text.slice(position, match.index)));
    const groups = match.groups;
    if (groups.code !== undefined) {
      out.push(`<code>${escapeText(groups.code)}</code>`);
    } else if (groups.imageAlt !== undefined) {
      out.push(escapeText(groups.imageAlt));
    } else if (groups.strong !== undefined) {
      out.push(`<strong>${inline(groups.strong)}</strong>`);
    } else if (groups.strike !== undefined) {
      out.push(`<del>${inline(groups.strike)}</del>`);
    } else if (groups.em !== undefined) {
      out.push(`<em>${inline(groups.em)}</em>`);
    } else {
      out.push(link(inline(groups.label), groups.url));
    }
    position = match.index + match[0].length;
  }
  out.push(escapeText(text.slice(position)));
  return out.join("");
}

function link(label, url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return linkText(label, url);
  }
  const scheme = parsed.protocol.replace(/:$/, "").toLowerCase();
  if (!SAFE_SCHEMES.has(scheme) || (scheme !== "mailto" && !parsed.host)) {
    return linkText(label, url);
  }
  return `<a href="${escapeAttribute(url)}" target="_blank" rel="noopener noreferrer">${label}</a>`;
}

function linkText(label, url) {
  return `${label} (${escapeText(url)})`;
}

function escapeText(text) {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function escapeAttribute(text) {
  return escapeText(text).replace(/"/g, "&quot;").replace(/'/g, "&#x27;");
}

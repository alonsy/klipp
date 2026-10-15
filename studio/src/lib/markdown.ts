// Just enough Markdown for Klipp's own release notes (bundled with the app):
// headings, paragraphs, lists, quotes, **bold**, `code` and [links](https://…).

function escape(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function inline(s: string): string {
  return escape(s)
    .replace(/\*\*(.+?)\*\*/g, "<b>$1</b>")
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\[([^\]]+)\]\((https?:[^)\s]+)\)/g, '<a href="$2">$1</a>');
}

export function renderMarkdown(md: string): string {
  const out: string[] = [];
  let paragraph: string[] = [];
  let list = false;
  const endParagraph = () => {
    if (paragraph.length) out.push(`<p>${inline(paragraph.join(" "))}</p>`);
    paragraph = [];
  };
  const endList = () => {
    if (list) out.push("</ul>");
    list = false;
  };
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trimEnd();
    const heading = /^(#{1,4})\s+(.*)$/.exec(line);
    const item = /^\s*[-*]\s+(.*)$/.exec(line);
    const quote = /^>\s?(.*)$/.exec(line);
    if (!line.trim()) {
      endParagraph();
      endList();
    } else if (heading) {
      endParagraph();
      endList();
      const level = Math.min(6, heading[1].length + 2);
      out.push(`<h${level}>${inline(heading[2])}</h${level}>`);
    } else if (item) {
      endParagraph();
      if (!list) out.push("<ul>");
      list = true;
      out.push(`<li>${inline(item[1])}</li>`);
    } else if (quote) {
      endParagraph();
      endList();
      out.push(`<blockquote>${inline(quote[1])}</blockquote>`);
    } else {
      endList();
      paragraph.push(line.trim());
    }
  }
  endParagraph();
  endList();
  return out.join("\n");
}

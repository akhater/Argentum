/**
 * The Changelog tab in Settings. Ours.
 *
 * There is no public repository yet, so the only place the history exists is
 * `CHANGELOG.md` in the working tree. This puts it in the app, where someone
 * running Argentum can actually read what changed, rather than nowhere.
 *
 * The file is imported at build time (`?raw`), so it ships inside the binary
 * and there is no file read, no path to get wrong, and nothing to fail at
 * runtime. It also means the changelog shown is exactly the one that built this
 * version.
 *
 * WHY A HAND-WRITTEN RENDERER
 *
 * No markdown library is in the dependency list, and adding one to render a
 * single file we write ourselves is a poor trade. `render` below handles the
 * subset our changelog actually uses — headings, lists, fenced code, tables,
 * bold and inline code — and anything else falls through as plain text rather
 * than breaking. If the changelog ever needs more, this grows; it is not
 * pretending to be a markdown implementation.
 */

import { useMemo } from 'react';
import changelog from '../../CHANGELOG.md?raw';

/** `**bold**` and `` `code` `` inside a line of prose. */
function inline(text: string, keyPrefix: string) {
  const parts: React.ReactNode[] = [];
  const pattern = /(\*\*[^*]+\*\*|`[^`]+`)/g;
  let last = 0;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > last) parts.push(text.slice(last, match.index));
    const token = match[0];
    const key = `${keyPrefix}-${match.index}`;
    if (token.startsWith('**')) {
      // Recurse, or `code` inside **bold** renders its backticks literally.
      parts.push(
        <strong key={key} className="font-semibold text-text-primary">
          {inline(token.slice(2, -2), `${key}-b`)}
        </strong>,
      );
    } else {
      parts.push(
        <code key={key} className="px-1 py-0.5 rounded bg-bg-primary text-accent text-[0.85em]">
          {token.slice(1, -1)}
        </code>,
      );
    }
    last = match.index + token.length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

/**
 * Drop the front matter and start at the first release.
 *
 * The top of CHANGELOG.md explains the versioning scheme and how to write an
 * entry. That is for whoever edits the file, not for whoever is running the
 * app, and it pushed the actual history below the fold. The "Based on RapidRAW"
 * line is kept: which upstream this build came from is worth seeing.
 */
function releasesOnly(markdown: string): string {
  const lines = markdown.split('\n');
  const first = lines.findIndex((l) => /^## \d{4}\.\d+\.\d+/.test(l));
  if (first === -1) {
    return markdown;
  }
  const basedOn = lines.find((l) => l.startsWith('**Based on'));
  return (basedOn ? `${basedOn}\n\n` : '') + lines.slice(first).join('\n');
}

function render(markdown: string): React.ReactNode[] {
  const out: React.ReactNode[] = [];
  const lines = markdown.split('\n');
  let i = 0;
  let list: string[] = [];

  const flushList = () => {
    if (list.length === 0) return;
    const items = list;
    list = [];
    out.push(
      <ul key={`ul-${out.length}`} className="my-2 space-y-1 list-disc ml-5 pl-1">
        {items.map((item, n) => (
          <li key={n}>{inline(item, `li-${out.length}-${n}`)}</li>
        ))}
      </ul>,
    );
  };

  while (i < lines.length) {
    const line = lines[i];

    // Fenced code, kept verbatim — the changelog uses it for measurements.
    // Matched on the trimmed line: inside a bullet the fence is indented, and
    // checking the raw line missed those, so every measurement in the file was
    // being swallowed into the bullet above it.
    if (line.trim().startsWith('```')) {
      const body: string[] = [];
      const indent = line.length - line.trimStart().length;
      i += 1;
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        body.push(lines[i].slice(indent));
        i += 1;
      }
      i += 1;
      flushList();
      out.push(
        <pre
          key={`pre-${out.length}`}
          className="my-3 p-3 rounded-lg bg-bg-primary overflow-x-auto text-xs leading-relaxed"
        >
          <code>{body.join('\n')}</code>
        </pre>,
      );
      continue;
    }

    // Tables: a header row, a separator, then rows.
    if (line.startsWith('|') && lines[i + 1]?.startsWith('|') && /^\|[\s:|-]+\|$/.test(lines[i + 1])) {
      const cells = (row: string) =>
        row.slice(1, -1).split('|').map((c) => c.trim());
      const head = cells(line);
      i += 2;
      const body: string[][] = [];
      while (i < lines.length && lines[i].startsWith('|')) {
        body.push(cells(lines[i]));
        i += 1;
      }
      flushList();
      out.push(
        <div key={`table-${out.length}`} className="my-3 overflow-x-auto">
          <table className="text-sm border-collapse">
            <thead>
              <tr>
                {head.map((h, n) => (
                  <th key={n} className="text-left font-semibold px-3 py-1.5 border-b border-surface">
                    {inline(h, `th-${n}`)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {body.map((row, r) => (
                <tr key={r}>
                  {row.map((c, n) => (
                    <td key={n} className="px-3 py-1.5 border-b border-surface/50 align-top">
                      {inline(c, `td-${r}-${n}`)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    if (line.startsWith('### ')) {
      flushList();
      out.push(
        <h4 key={`h4-${out.length}`} className="mt-4 mb-1 font-semibold text-text-primary">
          {line.slice(4)}
        </h4>,
      );
    } else if (line.startsWith('## ')) {
      flushList();
      out.push(
        <h3 key={`h3-${out.length}`} className="mt-8 mb-2 text-lg font-semibold text-accent">
          {line.slice(3)}
        </h3>,
      );
    } else if (line.startsWith('# ')) {
      flushList();
      // The document title is the tab's own title; skip it.
    } else if (line.startsWith('- ')) {
      // A bullet wraps too; indented continuation lines belong to it.
      let item = line.slice(2).trim();
      i += 1;
      while (
        i < lines.length
        && /^\s{2,}\S/.test(lines[i])
        && !lines[i].trim().startsWith('- ')
        && !lines[i].trim().startsWith('```')
      ) {
        item += ` ${lines[i].trim()}`;
        i += 1;
      }
      list.push(item);
      continue;
    } else if (line.trim() === '---') {
      flushList();
    } else if (line.trim() === '') {
      flushList();
    } else {
      // Markdown wraps a paragraph across lines; join them back, or every
      // source line becomes its own paragraph and prose breaks mid-sentence.
      flushList();
      const para: string[] = [];
      while (
        i < lines.length
        && lines[i].trim() !== ''
        && !lines[i].startsWith('#')
        && !lines[i].startsWith('- ')
        && !lines[i].startsWith('```')
        && !lines[i].startsWith('|')
        && lines[i].trim() !== '---'
      ) {
        para.push(lines[i].trim());
        i += 1;
      }
      const text = para.join(' ');
      out.push(
        <p key={`p-${out.length}`} className="my-2 leading-relaxed">
          {inline(text, `p-${out.length}`)}
        </p>,
      );
      continue;
    }
    i += 1;
  }

  flushList();
  return out;
}

export default function ChangelogPanel() {
  const body = useMemo(() => render(releasesOnly(changelog)), []);

  return (
    <div className="pb-6">
      <div className="p-6 bg-surface rounded-xl shadow-md">
        <h2 className="text-2xl font-semibold text-accent mb-4">Changelog</h2>
        <div className="text-text-secondary text-sm">{body}</div>
      </div>
    </div>
  );
}

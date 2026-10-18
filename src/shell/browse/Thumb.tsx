// A photo's thumbnail, as the grid and the filmstrip draw it: the image once
// the page holds it, its file type while it comes, a mark if none can be made.

import { ImageOff } from 'lucide-react';

import { text } from '../i18n';
import { useShell } from '../ShellContext';

const extensionOf = (path: string) => path.split('.').pop()?.toUpperCase() ?? '';

export function Thumb({ path }: { path: string }) {
  const url = useShell((s) => s.thumbUrl(path));
  const failed = useShell((s) => s.thumbFailed(path));
  if (url) return <img className="agb-thumb" src={url} alt="" draggable={false} decoding="async" />;
  return (
    <span className="agb-thumb is-empty" title={failed ? text('browse.noThumb') : undefined}>
      {failed ? <ImageOff size={18} /> : extensionOf(path)}
    </span>
  );
}

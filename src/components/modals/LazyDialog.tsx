import { Component, Suspense, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'react-toastify';

/**
 * Dialogs that are big and rarely opened load their code on first open
 * (`lazy(() => import(...))`), not at start-up. This renders the dialog only
 * once `isOpen` has been true, so the import is not requested before then.
 *
 * After the first open it stays mounted, exactly as the eager dialogs do: each
 * dialog plays its own closing animation and keeps its own state between
 * openings, so unmounting on close would cut those off. While the chunk loads
 * (a local file, a few milliseconds) nothing is drawn; the hidden marker
 * lets the UI walk wait for the load.
 *
 * A failed import (dev server only; the packaged app reads local files) or any
 * render error inside the dialog closes the dialog and shows an error toast
 * instead of blanking the app.
 */
export function LazyDialog({
  isOpen,
  onClose,
  children,
}: {
  isOpen: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const [armed, setArmed] = useState(isOpen);
  if (isOpen && !armed) setArmed(true);
  if (!armed) return null;

  return (
    <LoadFailure
      onFail={() => {
        setArmed(false);
        onClose();
        toast.error(t('modals.dialogError'));
      }}
    >
      <Suspense fallback={<span data-dialog-loading hidden />}>{children}</Suspense>
    </LoadFailure>
  );
}

class LoadFailure extends Component<{ onFail: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown) {
    console.error('Dialog failed to load', error);
    this.props.onFail();
  }

  render() {
    return this.state.failed ? null : this.props.children;
  }
}

import { useEffect, useRef, useState, useCallback } from 'react';
import { onceGate } from '../../argentum/singleRun';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import Button from '../ui/Button';
import Text from '../ui/Text';
import { TextVariants } from '../../types/typography';

interface ConfirmModalProps {
  cancelText?: string;
  confirmText?: string;
  confirmVariant?: string;
  focusCancel?: boolean;
  isOpen: boolean;
  message?: string;
  onClose(): void;
  onConfirm?(): void;
  title?: string;
}

export default function ConfirmModal({
  cancelText,
  confirmText,
  confirmVariant = 'primary',
  focusCancel = false,
  isOpen,
  message,
  onClose,
  onConfirm,
  title,
}: ConfirmModalProps) {
  const { t } = useTranslation();
  const [isMounted, setIsMounted] = useState(false);
  const [show, setShow] = useState(false);

  const resolvedCancelText = cancelText || t('modals.confirm.cancel');
  const resolvedConfirmText = confirmText || t('modals.confirm.confirm');

  // Argentum: the confirm acts once per opening, however often it is clicked (argentum/singleRun.ts).
  const confirmGate = useRef(onceGate()).current;

  useEffect(() => {
    if (isOpen) {
      confirmGate.reset();
      setIsMounted(true);
      const timer = setTimeout(() => {
        setShow(true);
      }, 10);
      return () => clearTimeout(timer);
    } else {
      setShow(false);
      const timer = setTimeout(() => {
        setIsMounted(false);
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [isOpen, confirmGate]);

  const handleConfirm = useCallback(() => {
    if (!confirmGate.claim()) return;
    try {
      onConfirm?.();
    } catch (err) {
      confirmGate.reset(); // the dialog stays open: let the next click try again
      throw err;
    }
    onClose();
  }, [onConfirm, onClose, confirmGate]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (e.key === 'Enter' && focusCancel) {
        // The focused button answers: Cancel unless the person moved to Delete.
        e.stopPropagation();
        e.nativeEvent.stopImmediatePropagation();
      } else if (e.key === 'Enter') {
        e.preventDefault();
        e.stopPropagation();
        e.nativeEvent.stopImmediatePropagation();
        handleConfirm();
      } else if (e.key === 'Escape') {
        e.preventDefault();
        e.stopPropagation();
        e.nativeEvent.stopImmediatePropagation();
        onClose();
      }
    },
    [handleConfirm, onClose, focusCancel],
  );

  if (!isMounted) {
    return null;
  }

  const modalContent = (
    <div
      aria-labelledby="confirm-modal-title"
      aria-modal="true"
      className={`
        fixed inset-0 flex items-center justify-center z-[9999]
        bg-black/30 backdrop-blur-xs
        transition-opacity duration-300 ease-in-out
        ${show ? 'opacity-100' : 'opacity-0'}
      `}
      onClick={onClose}
      role="dialog"
    >
      <div
        className={`
          bg-surface rounded-lg shadow-xl p-6 w-full max-w-md text-text-primary
          transform transition-all duration-300 ease-out
          ${show ? 'scale-100 opacity-100 translate-y-0' : 'scale-95 opacity-0 -translate-y-4'}
        `}
        onClick={(e: any) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <Text variant={TextVariants.title} id="confirm-modal-title" className="mb-4">
          {title}
        </Text>
        <Text className="mb-6 whitespace-pre-wrap">{message}</Text>
        <div className="flex justify-end gap-3 mt-5">
          <Button
            className="bg-bg-primary shadow-transparent hover:bg-bg-primary text-white shadow-none focus:outline-hidden focus:ring-0"
            onClick={onClose}
            variant="ghost"
            tabIndex={0}
            autoFocus={focusCancel}
          >
            {resolvedCancelText}
          </Button>
          <Button
            onClick={handleConfirm}
            variant={confirmVariant}
            autoFocus={!focusCancel}
            className="focus:outline-hidden focus:ring-0 focus:ring-offset-0"
          >
            {resolvedConfirmText}
          </Button>
        </div>
      </div>
    </div>
  );

  return typeof document !== 'undefined' ? createPortal(modalContent, document.body) : modalContent;
}

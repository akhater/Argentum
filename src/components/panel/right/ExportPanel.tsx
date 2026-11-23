import { useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { useTiffDepth } from '../../../argentum/exportDepth';
import { save, open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { FileInput, CheckCircle, XCircle, Loader, Ban, ChevronDown, ChevronRight, Settings, X } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import { useTranslation } from 'react-i18next';
import debounce from 'lodash.debounce';
import Switch from '../../ui/Switch';
import Button from '../../ui/Button';
import Dropdown from '../../ui/Dropdown';
import Slider from '../../ui/Slider';
import ImagePicker from '../../ui/ImagePicker';
import {
  BorderBasis,
  ExportPreset,
  ExportSettings,
  FileFormat,
  FILE_FORMATS,
  FILENAME_VARIABLES,
  Status,
  ExportState,
  FileFormats,
  MAX_EXPORT_DPI,
  WatermarkAnchor,
} from '../../ui/ExportImportProperties';
import { Invokes, SelectedImage, AppSettings } from '../../ui/AppProperties';
import ExportPresetsList from '../../ui/ExportPresetsList';
import { useExportSettings } from '../../../hooks/useExportSettings';
import Text from '../../ui/Text';
import { TextColors, TextVariants, TextWeights } from '../../../types/typography';
import { useEditorStore } from '../../../store/useEditorStore';
import { useLooks } from '../../../argentum/looks/looksStore';
import { useAgTranslation } from '../../../argentum/locales';
import { cropFrame } from '../../../argentum/linearEdges';
import { fileNameOf } from '../../../argentum/paths';
import { ag } from '../../../argentum/ag';
import { checkResize, resizedSizePx } from '../../../argentum/exportResize';

interface ExportPanelProps {
  exportState: ExportState;
  multiSelectedPaths: Array<string>;
  selectedImage: SelectedImage | null;
  setExportState(state: any): void;
  appSettings: AppSettings | null;
  onSettingsChange: (settings: AppSettings) => void;
  rootPaths: string[];
  isVisible?: boolean;
  /** Export the grid's selection (the Modern window's one use), not the open photo. */
  libraryContext?: boolean;
}

const MAX_PAD_RATIO = 1000;

function parseRatio(value: string): number | null {
  const parsed = Number(value.trim().replace(',', '.'));
  return Number.isFinite(parsed) && parsed > 0 && parsed <= MAX_PAD_RATIO ? parsed : null;
}

const MAX_BORDER_PERCENT = 100;

function parsePercent(value: string): number | null {
  const parsed = Number(value.trim().replace(',', '.'));
  return Number.isFinite(parsed) && parsed >= 0 && parsed <= MAX_BORDER_PERCENT ? parsed : null;
}

// Argentum (RapidRAW 1.6.6): the formats that can store a print resolution.
// WebP, JPEG XL and AVIF have no field for it, so they get no DPI row and no dpi.
const DPI_FORMATS: ReadonlyArray<string> = [FileFormats.Jpeg, FileFormats.Png, FileFormats.Tiff];

// The formats Keep metadata writes to. TIFF gets the camera EXIF but not the
// XMP rating, colour label and keywords, so its note leaves those out.
// RapidRAW 9e03d499: JPEG XL too.
const METADATA_FORMATS: ReadonlyArray<string> = [
  FileFormats.Jpeg,
  FileFormats.Png,
  FileFormats.Tiff,
  FileFormats.Webp,
  FileFormats.Jxl,
];

// From RapidRAW v1.6.6, src/components/panel/right/ExportPanel.tsx (parseDpi).
function parseDpi(value: string): number | null {
  const parsed = Number(value.trim());
  return Number.isInteger(parsed) && parsed > 0 && parsed <= MAX_EXPORT_DPI ? parsed : null;
}

const CM_PER_INCH = 2.54;

/** One printed edge in centimetres: a decimal under 10 cm, whole numbers above. */
function formatPrintCm(px: number, dpi: number): string {
  const cm = (px / dpi) * CM_PER_INCH;
  return cm < 10 ? cm.toFixed(1) : String(Math.round(cm));
}

function normalizeHexColor(value: string): string | null {
  const trimmed = value.trim();
  const hex = trimmed.startsWith('#') ? trimmed.slice(1) : trimmed;
  return /^[0-9a-fA-F]{6}$/.test(hex) ? `#${hex.toLowerCase()}` : null;
}

interface ParsedTextField<T> {
  text: string;
  parsed: T | null;
  handleChange: (text: string) => void;
  resetText: () => void;
}

function useParsedTextField<T>(
  value: T,
  onValidChange: (value: T) => void,
  parse: (text: string) => T | null,
): ParsedTextField<T> {
  const [text, setText] = useState<string>(String(value));

  useEffect(() => {
    setText(String(value));
  }, [value]);

  const handleChange = (next: string) => {
    setText(next);
    const parsed = parse(next);
    if (parsed !== null) onValidChange(parsed);
  };

  return { text, parsed: parse(text), handleChange, resetText: () => setText(String(value)) };
}

interface ValidatedNumberInputProps {
  ariaLabel: string;
  disabled: boolean;
  field: ParsedTextField<number>;
  max?: number;
}

function ValidatedNumberInput({ ariaLabel, disabled, field, max }: ValidatedNumberInputProps) {
  return (
    <input
      aria-label={ariaLabel}
      className={`w-20 bg-surface text-center rounded-md p-2 border focus:ring-accent text-text-secondary focus:text-text-primary ${
        field.parsed === null ? 'border-red-500' : 'border-surface focus:border-accent'
      }`}
      disabled={disabled}
      max={max}
      min="0"
      onChange={(e) => field.handleChange(e.target.value)}
      // RapidRAW 384f86f8: src/components/panel/right/ExportPanel.tsx
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur();
      }}
      step="any"
      type="number"
      value={field.text}
    />
  );
}

interface ColorFieldProps {
  color: string;
  disabled: boolean;
  field: ParsedTextField<string>;
  label: string;
  onColorChange: (color: string) => void;
}

function ColorField({ color, disabled, field, label, onColorChange }: ColorFieldProps) {
  return (
    <div>
      <Text variant={TextVariants.label} className="mb-2 block">
        {label}
      </Text>
      <div className="flex items-center gap-2 bg-surface p-2 rounded-md">
        <input
          aria-label={label}
          className="w-8 h-8 p-0 border-none rounded-sm cursor-pointer bg-transparent"
          disabled={disabled}
          onChange={(e) => onColorChange(e.target.value)}
          type="color"
          value={color}
        />
        <input
          className="w-full bg-bg-primary text-center rounded-md p-1 border border-surface focus:border-accent focus:ring-accent"
          disabled={disabled}
          onBlur={field.resetText}
          onChange={(e) => field.handleChange(e.target.value)}
          type="text"
          value={field.text}
        />
      </div>
    </div>
  );
}

interface SectionProps {
  children: any;
  title: string;
}

function Section({ title, children }: SectionProps) {
  return (
    <div>
      <Text variant={TextVariants.heading} className="mb-2">
        {title}
      </Text>
      <div className="space-y-2">{children}</div>
    </div>
  );
}

function WatermarkPreview({
  anchor,
  scale,
  spacing,
  opacity,
  watermarkPath,
  imageAspectRatio,
  watermarkImageAspectRatio,
}: {
  anchor: WatermarkAnchor;
  scale: number;
  spacing: number;
  opacity: number;
  watermarkPath: string | null;
  imageAspectRatio: number;
  watermarkImageAspectRatio: number;
}) {
  const { t } = useTranslation();

  const getPositionStyles = () => {
    const minDimPercent = imageAspectRatio > 1 ? 100 / imageAspectRatio : 100;
    const watermarkSizePercent = minDimPercent * (scale / 100);
    const spacingPercent = minDimPercent * (spacing / 100);

    const styles: React.CSSProperties = {
      width: `${watermarkSizePercent}%`,
      opacity: opacity / 100,
      position: 'absolute',
    };

    const spacingString = `${spacingPercent}%`;

    switch (anchor) {
      case WatermarkAnchor.TopLeft:
        styles.top = spacingString;
        styles.left = spacingString;
        break;
      case WatermarkAnchor.TopCenter:
        styles.top = spacingString;
        styles.left = '50%';
        styles.transform = 'translateX(-50%)';
        break;
      case WatermarkAnchor.TopRight:
        styles.top = spacingString;
        styles.right = spacingString;
        break;
      case WatermarkAnchor.CenterLeft:
        styles.top = '50%';
        styles.left = spacingString;
        styles.transform = 'translateY(-50%)';
        break;
      case WatermarkAnchor.Center:
        styles.top = '50%';
        styles.left = '50%';
        styles.transform = 'translate(-50%, -50%)';
        break;
      case WatermarkAnchor.CenterRight:
        styles.top = '50%';
        styles.right = spacingString;
        styles.transform = 'translateY(-50%)';
        break;
      case WatermarkAnchor.BottomLeft:
        styles.bottom = spacingString;
        styles.left = spacingString;
        break;
      case WatermarkAnchor.BottomCenter:
        styles.bottom = spacingString;
        styles.left = '50%';
        styles.transform = 'translateX(-50%)';
        break;
      case WatermarkAnchor.BottomRight:
        styles.bottom = spacingString;
        styles.right = spacingString;
        break;
    }
    return styles;
  };

  return (
    <div
      className="w-full bg-surface rounded-md relative overflow-hidden border border-surface"
      style={{ aspectRatio: imageAspectRatio }}
    >
      <div className="absolute inset-0 flex items-center justify-center">
        <Text variant={TextVariants.label}>{t('export.watermark.previewText')}</Text>
      </div>
      {watermarkPath && (
        <div style={getPositionStyles()}>
          <div
            className="w-full bg-accent/50 border-2 border-dashed border-accent rounded-xs flex items-center justify-center"
            style={{ aspectRatio: watermarkImageAspectRatio }}
          >
            <span className="text-white text-[8px] font-bold">{t('export.watermark.logoText')}</span>
          </div>
        </div>
      )}
    </div>
  );
}

const formatBytes = (bytes: number, t: any, decimals = 2) => {
  if (!+bytes) return `0 ${t('export.bytes.bytes')}`;
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = [
    t('export.bytes.bytes'),
    t('export.bytes.kb'),
    t('export.bytes.mb'),
    t('export.bytes.gb'),
    t('export.bytes.tb'),
  ];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`;
};

export default function ExportPanel({
  exportState,
  multiSelectedPaths,
  selectedImage,
  setExportState,
  appSettings,
  onSettingsChange,
  rootPaths,
  isVisible = true,
  libraryContext,
}: ExportPanelProps) {
  const { t } = useTranslation();
  const agT = useAgTranslation();

  const resizeModeOptions = useMemo(
    () => [
      { label: t('export.resize.modes.longEdge'), value: 'longEdge' },
      { label: t('export.resize.modes.shortEdge'), value: 'shortEdge' },
      { label: t('export.resize.modes.width'), value: 'width' },
      { label: t('export.resize.modes.height'), value: 'height' },
    ],
    [t],
  );

  const borderBasisOptions = useMemo(
    () => [
      { label: t('export.border.bases.longEdge'), value: BorderBasis.LongEdge },
      { label: t('export.border.bases.shortEdge'), value: BorderBasis.ShortEdge },
      { label: t('export.border.bases.eachEdge'), value: BorderBasis.EachEdge },
    ],
    [t],
  );

  const {
    fileFormat,
    setFileFormat,
    jpegQuality,
    setJpegQuality,
    dpi,
    setDpi,
    enableResize,
    setEnableResize,
    resizeMode,
    setResizeMode,
    resizeValue,
    setResizeValue,
    dontEnlarge,
    setDontEnlarge,
    enablePad,
    setEnablePad,
    padRatioWidth,
    setPadRatioWidth,
    padRatioHeight,
    setPadRatioHeight,
    padColor,
    setPadColor,
    enableBorder,
    setEnableBorder,
    borderBasis,
    setBorderBasis,
    borderHorizontalPercent,
    setBorderHorizontalPercent,
    borderVerticalPercent,
    setBorderVerticalPercent,
    borderColor,
    setBorderColor,
    keepMetadata,
    setKeepMetadata,
    preserveTimestamps,
    setPreserveTimestamps,
    stripGps,
    setStripGps,
    exportMasks,
    setExportMasks,
    filenameTemplate,
    setFilenameTemplate,
    enableWatermark,
    setEnableWatermark,
    watermarkPath,
    setWatermarkPath,
    watermarkAnchor,
    setWatermarkAnchor,
    watermarkScale,
    setWatermarkScale,
    watermarkSpacing,
    setWatermarkSpacing,
    watermarkOpacity,
    setWatermarkOpacity,
    preserveFolders,
    setPreserveFolders,
    handleApplyPreset,
    currentSettingsObject,
    destinationType,
    setDestinationType,
    subfolder,
    setSubfolder,
  } = useExportSettings();

  // Argentum: Looks > Save > As LUT file opens this tab set to CUBE.
  const wantLutExport = useLooks((s) => s.wantLutExport);
  useEffect(() => {
    if (!wantLutExport) return;
    setFileFormat(FileFormats.Cube);
    useLooks.setState({ wantLutExport: false });
  }, [wantLutExport, setFileFormat]);

  const adjustmentsRef = useRef(useEditorStore.getState().adjustments);

  const [isAdvancedExpanded, setIsAdvancedExpanded] = useState(false);
  const initDone = useRef(false);

  useEffect(() => {
    if (initDone.current || appSettings === null || !isVisible) return;
    initDone.current = true;
    const lastUsed = appSettings.exportPresets?.find((p) => p.id === '__last_used__');
    if (lastUsed) {
      handleApplyPreset(lastUsed);
    }
  }, [appSettings, handleApplyPreset, isVisible]);

  const saveLastUsedPreset = useCallback(
    (exportPath: string) => {
      if (!appSettings) return;
      const lastUsedPreset: ExportPreset = {
        ...currentSettingsObject,
        id: '__last_used__',
        name: '__last_used__',
        lastExportPath: exportPath,
      };
      const updatedPresets = [
        ...(appSettings.exportPresets ?? []).filter((p) => p.id !== '__last_used__'),
        lastUsedPreset,
      ];
      onSettingsChange({ ...appSettings, exportPresets: updatedPresets });
    },
    [appSettings, currentSettingsObject, onSettingsChange],
  );

  const padRatioWidthField = useParsedTextField(padRatioWidth, setPadRatioWidth, parseRatio);
  const padRatioHeightField = useParsedTextField(padRatioHeight, setPadRatioHeight, parseRatio);
  const padColorField = useParsedTextField(padColor, setPadColor, normalizeHexColor);

  const padRatioWidthValue = padRatioWidthField.parsed;
  const padRatioHeightValue = padRatioHeightField.parsed;
  const isPadValid = !enablePad || (padRatioWidthValue !== null && padRatioHeightValue !== null);
  const padSettings =
    enablePad && padRatioWidthValue !== null && padRatioHeightValue !== null
      ? { ratioWidth: padRatioWidthValue, ratioHeight: padRatioHeightValue, color: padColor }
      : null;

  const borderHorizontalField = useParsedTextField(borderHorizontalPercent, setBorderHorizontalPercent, parsePercent);
  const borderVerticalField = useParsedTextField(borderVerticalPercent, setBorderVerticalPercent, parsePercent);
  const borderColorField = useParsedTextField(borderColor, setBorderColor, normalizeHexColor);

  const borderHorizontalValue = borderHorizontalField.parsed;
  const borderVerticalValue = borderVerticalField.parsed;
  const isBorderValid = !enableBorder || (borderHorizontalValue !== null && borderVerticalValue !== null);
  const borderSettings =
    enableBorder && borderHorizontalValue !== null && borderVerticalValue !== null
      ? {
          basis: borderBasis,
          horizontalPercent: borderHorizontalValue,
          verticalPercent: borderVerticalValue,
          color: borderColor,
        }
      : null;

  const dpiField = useParsedTextField(dpi, setDpi, parseDpi);
  const supportsDpi = DPI_FORMATS.includes(fileFormat);

  const [estimatedSize, setEstimatedSize] = useState<number | null>(null);
  const tiffDepth = useTiffDepth();
  const [isEstimating, setIsEstimating] = useState<boolean>(false);
  const estimateRequestIdRef = useRef(0);
  const [watermarkImageAspectRatio, setWatermarkImageAspectRatio] = useState(1);
  const [imageAspectRatio, setImageAspectRatio] = useState(16 / 9);
  const filenameInputRef = useRef<HTMLInputElement>(null);

  const { status, progress, errorMessage } = exportState;
  const isExporting = [Status.Exporting, Status.Cancelling].includes(status);
  const isCancelling = status === Status.Cancelling;
  const isLibraryContext = !!libraryContext;

  const pathsToExport = useMemo(() => {
    return isLibraryContext
      ? multiSelectedPaths
      : multiSelectedPaths.length > 0
        ? multiSelectedPaths
        : selectedImage
          ? [selectedImage.path]
          : [];
  }, [isLibraryContext, multiSelectedPaths, selectedImage?.path]);

  const numImages = pathsToExport.length;

  // The most pixels an export writes, asked of Rust so the field and the export agree.
  const [exportPixelLimit, setExportPixelLimit] = useState<number | null>(null);
  useEffect(() => {
    ag('export_pixel_limit')
      .then(setExportPixelLimit)
      .catch(() => {});
  }, []);

  // The open photo's cropped size, which cropFrame reads from the editor store;
  // these subscriptions re-read it when the crop or a quarter turn changes.
  const editorCrop = useEditorStore((state) => state.adjustments.crop);
  const editorOrientationSteps = useEditorStore((state) => state.adjustments.orientationSteps);
  const exportFrame = useMemo(() => cropFrame(), [selectedImage, editorCrop, editorOrientationSteps]);

  const resizeSource =
    !isLibraryContext && numImages === 1 && exportFrame && pathsToExport[0] === selectedImage?.path
      ? { widthPx: Math.round(exportFrame.width), heightPx: Math.round(exportFrame.height) }
      : null;
  // With border or pad the canvas is worked out in Rust, which refuses an oversized one.
  const resizeCheck = enableResize
    ? checkResize(
        enableBorder || enablePad ? null : resizeSource,
        resizeMode,
        resizeValue,
        dontEnlarge,
        exportPixelLimit,
      )
    : 'ok';

  // Shown only when the size is known: one photo, the one open in the editor.
  // Border and pad change the canvas in ways only Rust works out, so the print
  // size is left out while either is on rather than shown wrong.
  const printSize = (() => {
    if (!supportsDpi || numImages !== 1 || enableBorder || enablePad) return null;
    if (!exportFrame || pathsToExport[0] !== selectedImage?.path) return null;
    let size = { widthPx: Math.round(exportFrame.width), heightPx: Math.round(exportFrame.height) };
    if (enableResize) {
      if (!(resizeValue > 0)) return null;
      size = resizedSizePx(size.widthPx, size.heightPx, resizeMode, resizeValue, dontEnlarge);
    }
    return agT('export166PrintsAt')
      .replace('{width}', formatPrintCm(size.widthPx, dpi))
      .replace('{height}', formatPrintCm(size.heightPx, dpi));
  })();

  useEffect(() => {
    const fetchDims = async () => {
      if (!enableWatermark || numImages === 0 || !isVisible) return;
      if (!isLibraryContext && selectedImage && selectedImage.width && selectedImage.height) {
        setImageAspectRatio(selectedImage.width / selectedImage.height);
        return;
      }
      try {
        const dims: any = await invoke('get_image_dimensions', { path: pathsToExport[0] });
        if (dims.width > 0 && dims.height > 0) setImageAspectRatio(dims.width / dims.height);
      } catch {
        setImageAspectRatio(3 / 2);
      }
    };
    fetchDims();
  }, [pathsToExport, isLibraryContext, selectedImage, enableWatermark, numImages, isVisible]);

  useEffect(() => {
    const fetchWatermarkDimensions = async () => {
      if (!watermarkPath) {
        setWatermarkImageAspectRatio(1);
        return;
      }
      try {
        const dimensions: { width: number; height: number } = await invoke('get_image_dimensions', {
          path: watermarkPath,
        });
        setWatermarkImageAspectRatio(dimensions.height > 0 ? dimensions.width / dimensions.height : 1);
      } catch (error) {
        setWatermarkImageAspectRatio(1);
      }
    };
    fetchWatermarkDimensions();
  }, [watermarkPath]);

  const anchorOptions = useMemo(
    () => [
      { label: t('export.watermark.anchors.topLeft'), value: WatermarkAnchor.TopLeft },
      { label: t('export.watermark.anchors.topCenter'), value: WatermarkAnchor.TopCenter },
      { label: t('export.watermark.anchors.topRight'), value: WatermarkAnchor.TopRight },
      { label: t('export.watermark.anchors.centerLeft'), value: WatermarkAnchor.CenterLeft },
      { label: t('export.watermark.anchors.center'), value: WatermarkAnchor.Center },
      { label: t('export.watermark.anchors.centerRight'), value: WatermarkAnchor.CenterRight },
      { label: t('export.watermark.anchors.bottomLeft'), value: WatermarkAnchor.BottomLeft },
      { label: t('export.watermark.anchors.bottomCenter'), value: WatermarkAnchor.BottomCenter },
      { label: t('export.watermark.anchors.bottomRight'), value: WatermarkAnchor.BottomRight },
    ],
    [t],
  );

  // RapidRAW f01c328c: src/components/panel/right/ExportPanel.tsx
  // Only the latest request's answer is shown; `null` means the open photo is
  // still loading, and the estimate runs again once it has (originalSize below).
  const debouncedEstimateSize = useMemo(
    () =>
      debounce(async (requestId, paths, currentAdj, currentPath, exportSettings, format) => {
        if (paths.length === 0 || !isVisible) {
          setEstimatedSize(null);
          setIsEstimating(false);
          return;
        }
        setIsEstimating(true);
        try {
          const size: number | null = await invoke(Invokes.EstimateExportSizes, {
            paths,
            exportSettings,
            outputFormat: format,
            currentEditPath: currentPath || null,
            currentEditAdjustments: currentAdj || null,
          });
          if (requestId !== estimateRequestIdRef.current || size === null) return;
          setEstimatedSize(size);
          setIsEstimating(false);
        } catch {
          if (requestId !== estimateRequestIdRef.current) return;
          setEstimatedSize(null);
          setIsEstimating(false);
        }
      }, 500),
    [isVisible],
  );

  useEffect(() => {
    const exportSettings: ExportSettings = {
      filenameTemplate,
      jpegQuality,
      keepMetadata,
      preserveTimestamps,
      preserveFolders,
      destinationType,
      subfolder,
      resize: enableResize ? { mode: resizeMode, value: resizeValue, dontEnlarge } : null,
      border: borderSettings,
      pad: padSettings,
      stripGps,
      exportMasks: exportMasks,
      watermark:
        enableWatermark && watermarkPath
          ? {
              path: watermarkPath,
              anchor: watermarkAnchor,
              scale: watermarkScale,
              spacing: watermarkSpacing,
              opacity: watermarkOpacity,
            }
          : null,
    };
    const format = FILE_FORMATS.find((f: FileFormat) => f.id === fileFormat)?.extensions[0] || 'jpeg';
    const runEstimate = () =>
      debouncedEstimateSize(
        ++estimateRequestIdRef.current,
        pathsToExport,
        adjustmentsRef.current,
        selectedImage?.path,
        exportSettings,
        format,
      );

    runEstimate();

    let { adjustments: prevAdjustments, originalSize: prevOriginalSize } = useEditorStore.getState();
    const unsubscribe = useEditorStore.subscribe((state) => {
      if (state.adjustments === prevAdjustments && state.originalSize === prevOriginalSize) return;
      prevAdjustments = state.adjustments;
      prevOriginalSize = state.originalSize;
      adjustmentsRef.current = state.adjustments;
      runEstimate();
    });

    return () => {
      unsubscribe();
      debouncedEstimateSize.cancel();
    };
  }, [
    pathsToExport,
    selectedImage?.path,
    fileFormat,
    jpegQuality,
    enableResize,
    resizeMode,
    resizeValue,
    dontEnlarge,
    enablePad,
    padRatioWidthField.text,
    padRatioHeightField.text,
    padColor,
    enableBorder,
    borderBasis,
    borderHorizontalField.text,
    borderVerticalField.text,
    borderColor,
    keepMetadata,
    preserveTimestamps,
    stripGps,
    filenameTemplate,
    enableWatermark,
    watermarkPath,
    watermarkAnchor,
    watermarkScale,
    watermarkSpacing,
    watermarkOpacity,
    debouncedEstimateSize,
    exportMasks,
    preserveFolders,
    isLibraryContext,
    tiffDepth,
  ]);

  const handleVariableClick = (variable: string) => {
    if (!filenameInputRef.current) return;
    const input: HTMLInputElement = filenameInputRef.current;
    const start = Number(input.selectionStart);
    const end = Number(input.selectionEnd);
    const currentValue = input.value;
    const newValue = currentValue.substring(0, start) + variable + currentValue.substring(end);
    setFilenameTemplate(newValue);
    setTimeout(() => {
      input.focus();
      input.setSelectionRange(start + variable.length, start + variable.length);
    }, 0);
  };

  const handleExport = async () => {
    if (numImages === 0 || isExporting) return;

    const exportSettings: ExportSettings = {
      filenameTemplate,
      jpegQuality,
      keepMetadata,
      preserveTimestamps,
      preserveFolders,
      destinationType,
      subfolder,
      resize: enableResize ? { mode: resizeMode, value: resizeValue, dontEnlarge } : null,
      border: borderSettings,
      pad: padSettings,
      stripGps,
      exportMasks: exportMasks,
      watermark:
        enableWatermark && watermarkPath
          ? {
              path: watermarkPath,
              anchor: watermarkAnchor,
              scale: watermarkScale,
              spacing: watermarkSpacing,
              opacity: watermarkOpacity,
            }
          : null,
      dpi: supportsDpi ? dpi : undefined,
    };

    const lastExportPath = appSettings?.exportPresets?.find((p) => p.id === '__last_used__')?.lastExportPath;

    try {
      const selectedFormat: any = FILE_FORMATS.find((f) => f.id === fileFormat);

      let outputFolderOrFile = '';
      const isOriginalFolder = destinationType === 'originalFolder';
      const shouldChooseOutputFile = numImages === 1 && !preserveFolders && !isOriginalFolder;

      if (isOriginalFolder) {
        outputFolderOrFile = 'originalFolderDummy';
      } else if (shouldChooseOutputFile) {
        const originalFilename = fileNameOf(pathsToExport[0]);
        const stem = originalFilename.substring(0, originalFilename.lastIndexOf('.')) || originalFilename;
        const suggestedName = (filenameTemplate || '').replace('{original_filename}', stem);
        const outputFileName = `${suggestedName}.${selectedFormat.extensions[0]}`;

        outputFolderOrFile = (await save({
          title: t('export.dialog.saveEditedImageTitle'),
          defaultPath: lastExportPath ? `${lastExportPath}/${outputFileName}` : outputFileName,
          filters: [
            { name: selectedFormat.name, extensions: selectedFormat.extensions },
            ...FILE_FORMATS.filter((f: FileFormat) => f.id !== fileFormat).map((f: FileFormat) => ({
              name: f.name,
              extensions: f.extensions,
            })),
          ],
        })) as string;
      } else {
        outputFolderOrFile = (await open({
          title: t('export.dialog.selectFolderTitle', { count: numImages }),
          directory: true,
          defaultPath: lastExportPath ?? undefined,
        })) as string;
      }

      if (outputFolderOrFile) {
        if (isOriginalFolder) {
          saveLastUsedPreset(lastExportPath || '');
        } else {
          const dir = shouldChooseOutputFile
            ? outputFolderOrFile.substring(
                0,
                Math.max(outputFolderOrFile.lastIndexOf('/'), outputFolderOrFile.lastIndexOf('\\')),
              )
            : outputFolderOrFile;
          if (dir) saveLastUsedPreset(dir);
        }

        setExportState({ status: Status.Exporting, progress: { current: 0, total: numImages }, errorMessage: '' });
        await invoke(Invokes.ExportImages, {
          paths: pathsToExport,
          outputFolderOrFile: outputFolderOrFile,
          isExplicitFilePath: shouldChooseOutputFile,
          baseOriginFolders: rootPaths,
          exportSettings,
          outputFormat: selectedFormat.extensions[0],
          currentEditPath: selectedImage?.path || null,
          currentEditAdjustments: adjustmentsRef.current || null,
        });
      }
    } catch (error) {
      setExportState({
        errorMessage: typeof error === 'string' ? error : t('export.status.failed'),
        progress,
        status: Status.Error,
      });
    }
  };

  const handleCancel = async () => {
    setExportState((current: ExportState) =>
      current.status === Status.Exporting ? { status: Status.Cancelling } : {},
    );
    try {
      await invoke(Invokes.CancelExport);
    } catch (error) {
      console.error('Failed to cancel:', error);
      setExportState((current: ExportState) =>
        current.status === Status.Cancelling ? { status: Status.Exporting } : {},
      );
    }
  };

  const canExport = numImages > 0 && isPadValid && isBorderValid && resizeCheck === 'ok';
  const isLut = fileFormat === FileFormats.Cube;
  const itemLabel = isLut ? t('export.labels.lut') : t('export.labels.image');
  const itemLabelPlural = isLut ? t('export.labels.lut_plural') : t('export.labels.image_plural');

  return (
    <div className={libraryContext ? 'h-full bg-bg-secondary rounded-lg flex flex-col' : 'flex flex-col h-full'}>
      <div className="p-3 flex justify-between items-center shrink-0 border-b border-surface">
        <Text variant={TextVariants.title}>{t('export.title')}</Text>
      </div>
      <div className="grow overflow-y-auto p-3 space-y-8">
        {/* RapidRAW 76ab6bb2: an invalid border or padding value disables Export, not the panel. */}
        {numImages > 0 ? (
          <>
            <div className={isExporting ? 'opacity-50 pointer-events-none' : ''}>
              <ExportPresetsList
                appSettings={appSettings}
                onSettingsChange={onSettingsChange}
                currentSettings={currentSettingsObject}
                onApplyPreset={handleApplyPreset}
              />
            </div>

            <Section title={t('export.sections.fileSettings')}>
              <div className="grid grid-cols-3 gap-2">
                {FILE_FORMATS.map((format: FileFormat) => (
                  <button
                    className={`px-2 py-1.5 rounded-md transition-colors ${fileFormat === format.id ? 'bg-accent' : 'bg-surface hover:bg-card-active'} disabled:opacity-50`}
                    disabled={isExporting}
                    key={format.id}
                    onClick={() => setFileFormat(format.id)}
                  >
                    <Text color={fileFormat === format.id ? TextColors.button : TextColors.secondary}>
                      {format.name}
                    </Text>
                  </button>
                ))}
              </div>
              {fileFormat === FileFormats.Tiff && <div data-argentum="export-precision" />}
              {[FileFormats.Jpeg, FileFormats.Webp, FileFormats.Jxl].includes(fileFormat as FileFormats) && (
                <div className={isExporting ? 'opacity-50 pointer-events-none' : ''}>
                  <Slider
                    defaultValue={90}
                    label={
                      fileFormat === FileFormats.Jxl && jpegQuality === 100
                        ? t('export.file.qualityLossless')
                        : t('export.file.quality')
                    }
                    max={100}
                    min={1}
                    onChange={(e) => setJpegQuality(Number(e.target.value))}
                    step={1}
                    value={jpegQuality}
                    fillOrigin="min"
                  />
                </div>
              )}
            </Section>

            <Section title={t('export.sections.destination')}>
              <div className="space-y-3">
                <Dropdown
                  options={[
                    { label: t('export.destination.customFolder'), value: 'customFolder' },
                    { label: t('export.destination.originalFolder'), value: 'originalFolder' },
                  ]}
                  value={destinationType || 'customFolder'}
                  onChange={(val) => setDestinationType(val as string)}
                  disabled={isExporting}
                  className="w-full"
                />

                {destinationType === 'originalFolder' && (
                  <div className="flex items-center gap-3 pl-2 border-l-2 border-surface">
                    <Text variant={TextVariants.label} className="whitespace-nowrap min-w-[70px]">
                      {t('export.destination.subfolder')}
                    </Text>
                    <input
                      className="w-full bg-surface border border-transparent rounded-md px-3 py-2 text-sm text-text-primary focus:outline-hidden truncate"
                      disabled={isExporting}
                      onChange={(e) => setSubfolder(e.target.value)}
                      type="text"
                      value={subfolder || ''}
                      placeholder={t('export.destination.subfolderPlaceholder')}
                    />
                  </div>
                )}
              </div>
            </Section>

            {numImages > 1 && (
              <Section title={t('export.sections.fileNaming')}>
                <input
                  className="w-full bg-surface border border-surface rounded-md p-2 text-sm text-text-primary focus:ring-accent focus:border-accent"
                  disabled={isExporting}
                  onChange={(e) => setFilenameTemplate(e.target.value)}
                  ref={filenameInputRef}
                  type="text"
                  value={filenameTemplate}
                />
                <div className="flex flex-wrap gap-2 mt-2">
                  {FILENAME_VARIABLES.map((variable: string) => (
                    <button
                      className="px-2 py-1 bg-surface text-text-secondary text-xs rounded-md hover:bg-card-active transition-colors disabled:opacity-50"
                      data-tooltip={
                        variable === '{original_filename_number}' ? agT('export166FrameNumberTip') : undefined
                      }
                      disabled={isExporting}
                      key={variable}
                      onClick={() => handleVariableClick(variable)}
                    >
                      {variable}
                    </button>
                  ))}
                </div>
              </Section>
            )}

            {fileFormat !== FileFormats.Cube && (
              <>
                <Section title={t('export.sections.imageSizing')}>
                  <Switch
                    label={t('export.border.addBorder')}
                    checked={enableBorder}
                    onChange={setEnableBorder}
                    disabled={isExporting}
                    trackClassName="bg-surface"
                  />
                  {enableBorder && (
                    <div className="space-y-4 pl-2 border-l-2 border-surface">
                      <div>
                        <Text variant={TextVariants.label} className="mb-2 block">
                          {t('export.border.basis')}
                        </Text>
                        <Dropdown
                          options={borderBasisOptions}
                          value={borderBasis}
                          onChange={setBorderBasis}
                          disabled={isExporting}
                          className="w-full"
                        />
                      </div>
                      <div className="flex items-center gap-2">
                        <Text variant={TextVariants.label} className="flex-1">
                          {t('export.border.leftRight')}
                        </Text>
                        <ValidatedNumberInput
                          ariaLabel={t('export.border.leftRight')}
                          disabled={isExporting}
                          field={borderHorizontalField}
                          max={MAX_BORDER_PERCENT}
                        />
                        <Text variant={TextVariants.label}>{t('export.border.percent')}</Text>
                      </div>
                      <div className="flex items-center gap-2">
                        <Text variant={TextVariants.label} className="flex-1">
                          {t('export.border.topBottom')}
                        </Text>
                        <ValidatedNumberInput
                          ariaLabel={t('export.border.topBottom')}
                          disabled={isExporting}
                          field={borderVerticalField}
                          max={MAX_BORDER_PERCENT}
                        />
                        <Text variant={TextVariants.label}>{t('export.border.percent')}</Text>
                      </div>
                      {!isBorderValid && (
                        <Text variant={TextVariants.label} className="text-red-500">
                          {t('export.border.invalidPercent')}
                        </Text>
                      )}
                      <ColorField
                        color={borderColor}
                        disabled={isExporting}
                        field={borderColorField}
                        label={t('export.border.color')}
                        onColorChange={setBorderColor}
                      />
                    </div>
                  )}
                  <Switch
                    label={t('export.pad.padToAspectRatio')}
                    checked={enablePad}
                    onChange={setEnablePad}
                    disabled={isExporting}
                    trackClassName="bg-surface"
                  />
                  {enablePad && (
                    <div className="space-y-4 pl-2 border-l-2 border-surface">
                      <div className="flex items-center gap-2">
                        <Text variant={TextVariants.label} className="flex-1">
                          {t('export.pad.aspectRatio')}
                        </Text>
                        <ValidatedNumberInput
                          ariaLabel={t('export.pad.ratioWidth')}
                          disabled={isExporting}
                          field={padRatioWidthField}
                        />
                        <Text variant={TextVariants.label}>:</Text>
                        <ValidatedNumberInput
                          ariaLabel={t('export.pad.ratioHeight')}
                          disabled={isExporting}
                          field={padRatioHeightField}
                        />
                      </div>
                      {!isPadValid && (
                        <Text variant={TextVariants.label} className="text-red-500">
                          {t('export.pad.invalidRatio')}
                        </Text>
                      )}
                      <ColorField
                        color={padColor}
                        disabled={isExporting}
                        field={padColorField}
                        label={t('export.pad.backgroundColor')}
                        onColorChange={setPadColor}
                      />
                    </div>
                  )}
                  <Switch
                    label={t('export.resize.resizeToFit')}
                    checked={enableResize}
                    onChange={setEnableResize}
                    disabled={isExporting}
                    trackClassName="bg-surface"
                  />
                  {enableResize && (
                    <div className="space-y-4 pl-2 border-l-2 border-surface">
                      <div className="flex items-center gap-2">
                        <Dropdown
                          options={resizeModeOptions}
                          value={resizeMode}
                          onChange={setResizeMode}
                          disabled={isExporting}
                          className="w-full"
                        />
                        <input
                          className="w-24 bg-surface text-center rounded-md p-2 border border-surface focus:border-accent focus:ring-accent text-text-secondary focus:text-text-primary"
                          disabled={isExporting}
                          min="1"
                          onChange={(e) => setResizeValue(parseInt(e?.target?.value))}
                          type="number"
                          value={resizeValue}
                        />
                        <Text variant={TextVariants.label}>{t('export.resize.pixels')}</Text>
                      </div>
                      {resizeCheck === 'invalid' && (
                        <Text variant={TextVariants.label} className="text-red-500">
                          {t('export.resize.invalid')}
                        </Text>
                      )}
                      {resizeCheck === 'overLimit' && exportPixelLimit !== null && (
                        <Text variant={TextVariants.label} className="text-red-500">
                          {t('export.resize.overLimit', { limit: exportPixelLimit / 1_000_000 })}
                        </Text>
                      )}
                      <Switch
                        checked={dontEnlarge}
                        disabled={isExporting}
                        label={t('export.resize.dontEnlarge')}
                        onChange={setDontEnlarge}
                        trackClassName="bg-surface"
                      />
                    </div>
                  )}
                  {supportsDpi && (
                    <div className="flex items-center gap-2">
                      <div className="flex-1 flex flex-col">
                        <Text
                          variant={TextVariants.label}
                          className="cursor-default"
                          data-tooltip={agT('export166ResolutionTip')}
                        >
                          {agT('export166Resolution')}
                        </Text>
                        {printSize && <Text variant={TextVariants.small}>{printSize}</Text>}
                      </div>
                      <ValidatedNumberInput
                        ariaLabel={agT('export166Resolution')}
                        disabled={isExporting}
                        field={dpiField}
                        max={MAX_EXPORT_DPI}
                      />
                      <Text variant={TextVariants.label}>{agT('export166Dpi')}</Text>
                    </div>
                  )}
                </Section>

                {METADATA_FORMATS.includes(fileFormat) && (
                  <Section title={t('export.sections.metadata')}>
                    <Switch
                      checked={keepMetadata}
                      disabled={isExporting}
                      label={t('export.metadata.saveWithMetadata')}
                      onChange={setKeepMetadata}
                      trackClassName="bg-surface"
                    />
                    {keepMetadata && (
                      <Text variant={TextVariants.small}>
                        {fileFormat === FileFormats.Tiff ? agT('export166MetadataTiff') : agT('export166Metadata')}
                      </Text>
                    )}
                    {keepMetadata && (
                      <div className="pl-2 border-l-2 border-surface">
                        <Switch
                          label={t('export.metadata.removeGps')}
                          checked={stripGps}
                          onChange={setStripGps}
                          disabled={isExporting}
                          trackClassName="bg-surface"
                        />
                      </div>
                    )}
                  </Section>
                )}

                <Section title={t('export.sections.watermark')}>
                  <Switch
                    label={t('export.watermark.addWatermark')}
                    checked={enableWatermark}
                    onChange={setEnableWatermark}
                    disabled={isExporting}
                    trackClassName="bg-surface"
                  />
                  {enableWatermark && (
                    <div className="space-y-4 pl-2 border-l-2 border-surface">
                      <div className={isExporting ? 'opacity-50 pointer-events-none' : ''}>
                        <ImagePicker
                          label={t('export.watermark.watermarkImage')}
                          imageName={watermarkPath ? fileNameOf(watermarkPath) || null : null}
                          onImageSelect={setWatermarkPath}
                          onClear={() => setWatermarkPath(null)}
                        />
                      </div>
                      {watermarkPath && (
                        <>
                          <Dropdown
                            options={anchorOptions}
                            value={watermarkAnchor}
                            onChange={(val) => setWatermarkAnchor(val as WatermarkAnchor)}
                            disabled={isExporting}
                            className="w-full"
                          />
                          <div>
                            <Slider
                              label={t('export.watermark.scale')}
                              min={1}
                              max={100}
                              step={1}
                              value={watermarkScale}
                              onChange={(e) => setWatermarkScale(Number(e.target.value))}
                              disabled={isExporting}
                              defaultValue={10}
                            />
                            <Slider
                              label={t('export.watermark.spacing')}
                              min={0}
                              max={25}
                              step={1}
                              value={watermarkSpacing}
                              onChange={(e) => setWatermarkSpacing(Number(e.target.value))}
                              disabled={isExporting}
                              defaultValue={5}
                            />
                            <Slider
                              label={t('export.watermark.opacity')}
                              min={0}
                              max={100}
                              step={1}
                              value={watermarkOpacity}
                              onChange={(e) => setWatermarkOpacity(Number(e.target.value))}
                              disabled={isExporting}
                              defaultValue={75}
                            />
                          </div>
                          <WatermarkPreview
                            imageAspectRatio={imageAspectRatio}
                            watermarkImageAspectRatio={watermarkImageAspectRatio}
                            watermarkPath={watermarkPath}
                            anchor={watermarkAnchor as WatermarkAnchor}
                            scale={watermarkScale}
                            spacing={watermarkSpacing}
                            opacity={watermarkOpacity}
                          />
                        </>
                      )}
                    </div>
                  )}
                </Section>
              </>
            )}

            <div>
              <Text variant={TextVariants.heading} className="mb-2">
                {t('export.sections.advanced')}
              </Text>
              <div className="bg-surface rounded-xl overflow-hidden">
                <button
                  onClick={() => setIsAdvancedExpanded(!isAdvancedExpanded)}
                  disabled={isExporting}
                  className="w-full flex items-center justify-between p-3.5 hover:bg-card-active transition-colors"
                >
                  <Text
                    as="span"
                    variant={TextVariants.label}
                    color={TextColors.primary}
                    className="flex items-center gap-2"
                  >
                    <Settings size={16} /> {t('export.advanced.title')}
                  </Text>
                  <Text color={TextColors.secondary}>
                    {isAdvancedExpanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                  </Text>
                </button>
                <AnimatePresence initial={false}>
                  {isAdvancedExpanded && (
                    <motion.div
                      initial={{ height: 0, opacity: 0 }}
                      animate={{ height: 'auto', opacity: 1 }}
                      exit={{ height: 0, opacity: 0 }}
                      transition={{ duration: 0.25, ease: [0.4, 0, 0.2, 1] }}
                      className="overflow-hidden"
                    >
                      <div className="px-4 pb-4 pt-2 border-t border-surface/50 flex flex-col gap-4">
                        <Switch
                          label={t('export.advanced.preserveFolders')}
                          checked={preserveFolders}
                          onChange={setPreserveFolders}
                          disabled={isExporting}
                        />
                        {fileFormat !== FileFormats.Cube && (
                          <>
                            <Switch
                              checked={preserveTimestamps}
                              disabled={isExporting}
                              label={t('export.advanced.preserveTimestamps')}
                              onChange={setPreserveTimestamps}
                            />
                            <Switch
                              label={t('export.advanced.exportMasks')}
                              checked={exportMasks}
                              onChange={setExportMasks}
                              disabled={isExporting}
                            />
                          </>
                        )}
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </div>
            </div>
          </>
        ) : (
          <div className="flex items-center justify-center h-full">
            <Text
              variant={TextVariants.heading}
              color={TextColors.secondary}
              weight={TextWeights.normal}
              className="text-center"
            >
              {isLibraryContext ? t('export.status.noImagesSelected') : t('export.status.noImageSelected')}
            </Text>
          </div>
        )}
      </div>

      <div className="p-3 border-t border-surface shrink-0 space-y-2">
        <Text as="div" variant={TextVariants.small} color={TextColors.primary} className="text-center">
          {isEstimating ? (
            <span className="italic">{t('export.status.estimatingSize')}</span>
          ) : estimatedSize !== null ? (
            <span>
              {numImages > 1
                ? t('export.status.estimatedTotalSize', { size: formatBytes(estimatedSize, t) }) ||
                  t('export.status.estimatedSize', { size: formatBytes(estimatedSize, t) })
                : t('export.status.estimatedSize', { size: formatBytes(estimatedSize, t) })}
              {numImages > 1 && ` (~${formatBytes(estimatedSize / numImages, t)})`}
            </span>
          ) : null}
        </Text>
        <motion.div
          whileTap={!(isCancelling || (status !== Status.Exporting && !canExport)) ? { scale: 0.98 } : undefined}
          transition={{ type: 'spring', stiffness: 400, damping: 17 }}
          className="w-full"
        >
          <Button
            className={`group rounded-md h-11 w-full flex items-center text-md font-bold! justify-center transition-colors ${
              status === Status.Exporting
                ? 'bg-red-500/15 hover:bg-red-500/25 text-red-400 border border-red-500/30 shadow-none'
                : status === Status.Cancelling
                  ? 'bg-yellow-500/20 text-yellow-400 shadow-none'
                  : status === Status.Success
                    ? 'bg-green-500/70 text-white shadow-none'
                    : status === Status.Error
                      ? 'bg-red-500/20 text-red-400 shadow-none'
                      : status === Status.Cancelled
                        ? 'bg-yellow-500/20 text-yellow-400 shadow-none'
                        : ''
            }`}
            disabled={isCancelling || (status !== Status.Exporting && !canExport)}
            onClick={status === Status.Exporting ? handleCancel : handleExport}
            size="lg"
          >
            {status === Status.Exporting ? (
              <>
                <span className="flex items-center group-hover:hidden">
                  <Loader size={18} className="animate-spin mr-2" />
                  {progress.total > 1
                    ? t('export.status.exportingProgress', { current: progress.current, total: progress.total })
                    : t('export.status.exporting')}
                </span>
                <span className="hidden items-center group-hover:flex">
                  <X size={18} className="mr-2" />
                  {t('export.status.cancelExport')}
                </span>
              </>
            ) : status === Status.Cancelling ? (
              <>
                <Loader size={18} className="animate-spin mr-2" /> {t('export.status.cancelling')}
              </>
            ) : status === Status.Success ? (
              <>
                <CheckCircle size={18} className="mr-2" /> {t('export.status.success')}
              </>
            ) : status === Status.Error ? (
              <>
                <XCircle size={18} className="mr-2" /> {errorMessage || t('export.status.failed')}
              </>
            ) : status === Status.Cancelled ? (
              <>
                <Ban size={18} className="mr-2" /> {t('export.status.cancelled')}
              </>
            ) : (
              <>
                <FileInput size={18} className="mr-2" />{' '}
                {numImages > 1
                  ? t('export.status.exportMultiple', { count: numImages, label: itemLabelPlural })
                  : t('export.status.exportSingle', { label: itemLabel })}
              </>
            )}
          </Button>
        </motion.div>
      </div>
    </div>
  );
}

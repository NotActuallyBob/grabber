<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

type Selection = {
  left: number;
  top: number;
  width: number;
  height: number;
};

type ScreenCapture = {
  data_url: string;
  x: number;
  y: number;
  width: number;
  height: number;
};

type AnnotationPoint = {
  x: number;
  y: number;
};

type AnnotationStroke = {
  kind: "stroke";
  points: AnnotationPoint[];
  color: string;
  width: number;
  order: number;
};

type AnnotationRectangle = {
  kind: "rectangle" | "oval" | "line" | "arrow" | "triangle";
  start: AnnotationPoint;
  end: AnnotationPoint;
  color: string;
  width: number;
  fill: boolean;
  order: number;
};

type AnnotationItem = AnnotationStroke | AnnotationRectangle;

const currentWindow = getCurrentWindow();
const isCaptureWindow = currentWindow.label === "capture";
const selectionImage = ref("");
const screenshot = ref("");
const screenshotZoom = ref(1);
const screenshotPanX = ref(0);
const screenshotPanY = ref(0);
const isDrawingMode = ref(false);
const brushMenuOpen = ref(false);
const brushColor = ref("#e53935");
const brushWidth = ref(4);
const fillShapes = ref(false);
const shapeMenuOpen = ref(false);
const isShapeMode = ref(false);
const selectedShape = ref<AnnotationRectangle["kind"]>("rectangle");
const annotationRectangles = ref<AnnotationRectangle[]>([]);
const currentRectangle = ref<AnnotationRectangle | null>(null);
const annotationPaths = ref<AnnotationStroke[]>([]);
const currentAnnotation = ref<AnnotationPoint[]>([]);
const annotationOrder = ref(0);
const contextMenu = ref({ visible: false, x: 0, y: 0 });
const isPanningScreenshot = ref(false);
const screenshotPanStart = ref({ x: 0, y: 0 });
const screenshotPanOrigin = ref({ x: 0, y: 0 });
const isSelecting = ref(false);
const isDragging = ref(false);
const selection = ref<Selection | null>(null);
const selectionSurface = ref<HTMLElement | null>(null);
const screenshotViewer = ref<HTMLElement | null>(null);
const screenshotImage = ref<HTMLImageElement | null>(null);
const startPoint = ref({ x: 0, y: 0 });
let unlistenCaptureStart: UnlistenFn | undefined;
let unlistenCaptureComplete: UnlistenFn | undefined;

const orderedAnnotations = computed<AnnotationItem[]>(() => [
  ...annotationPaths.value,
  ...annotationRectangles.value,
].sort((first, second) => first.order - second.order));

const selectionStyle = computed(() => {
  if (!selection.value) {
    return {};
  }

  return {
    left: `${selection.value.left}px`,
    top: `${selection.value.top}px`,
    width: `${selection.value.width}px`,
    height: `${selection.value.height}px`,
  };
});

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(Math.max(value, minimum), maximum);
}

function showScreenshot(image: string) {
  screenshot.value = image;
  screenshotZoom.value = 1;
  screenshotPanX.value = 0;
  screenshotPanY.value = 0;
  annotationPaths.value = [];
  annotationRectangles.value = [];
  annotationOrder.value = 0;
  currentAnnotation.value = [];
  currentRectangle.value = null;
  isDrawingMode.value = false;
  isShapeMode.value = false;
  selectedShape.value = "rectangle";
}

async function cancelSelection() {
  if (!isCaptureWindow) {
    return;
  }

  selectionImage.value = "";
  selection.value = null;
  isDragging.value = false;
  isSelecting.value = false;
  localStorage.removeItem("grabber.pendingCapture");
  await currentWindow.hide();
}

function onEscape(event: KeyboardEvent) {
  if (event.key === "Escape" && isSelecting.value) {
    void cancelSelection();
  }
}

async function beginSelection() {
  if (isCaptureWindow) {
    return;
  }

  try {
    const capture = await invoke<ScreenCapture>("capture_screen");
    localStorage.setItem("grabber.pendingCapture", JSON.stringify(capture));
    let captureWindow = await WebviewWindow.getByLabel("capture");
    if (!captureWindow) {
      const createdWindow = new WebviewWindow("capture", {
        url: "/",
        visible: false,
        decorations: false,
        resizable: false,
        skipTaskbar: true,
        alwaysOnTop: true,
        shadow: false,
        width: 1,
        height: 1,
      });
      await new Promise<void>((resolve, reject) => {
        createdWindow.once("tauri://created", () => resolve());
        createdWindow.once("tauri://error", (event) => reject(event));
      });
      captureWindow = createdWindow;
    }

    await captureWindow.setPosition(new PhysicalPosition(capture.x, capture.y));
    await captureWindow.setSize(new PhysicalSize(capture.width, capture.height));
    await captureWindow.show();
    await captureWindow.setFocus();
    await emitTo("capture", "capture-start", capture);
  } catch (error) {
    console.error("Unable to start screen capture", error);
    localStorage.removeItem("grabber.pendingCapture");
  }
}

function updateSelection(event: PointerEvent) {
  const surface = selectionSurface.value;
  if (!surface || !isSelecting.value || !isDragging.value) {
    return;
  }

  const bounds = surface.getBoundingClientRect();
  const currentX = clamp(event.clientX - bounds.left, 0, bounds.width);
  const currentY = clamp(event.clientY - bounds.top, 0, bounds.height);
  const left = Math.min(startPoint.value.x, currentX);
  const top = Math.min(startPoint.value.y, currentY);

  selection.value = {
    left,
    top,
    width: Math.abs(currentX - startPoint.value.x),
    height: Math.abs(currentY - startPoint.value.y),
  };
}

function startDrag(event: PointerEvent) {
  if (event.button !== 0 || !event.isPrimary) {
    return;
  }

  const surface = event.currentTarget as HTMLElement;
  selectionSurface.value = surface;
  const bounds = surface.getBoundingClientRect();
  startPoint.value = {
    x: clamp(event.clientX - bounds.left, 0, bounds.width),
    y: clamp(event.clientY - bounds.top, 0, bounds.height),
  };
  selection.value = {
    left: startPoint.value.x,
    top: startPoint.value.y,
    width: 0,
    height: 0,
  };
  isDragging.value = true;
  surface.setPointerCapture(event.pointerId);
}

async function completeDrag(event: PointerEvent) {
  const surface = selectionSurface.value;
  if (!surface || !selection.value || !isDragging.value) {
    return;
  }

  updateSelection(event);
  surface.releasePointerCapture(event.pointerId);
  const chosen = selection.value;
  isDragging.value = false;
  if (chosen.width < 2 || chosen.height < 2) {
    selection.value = null;
    return;
  }

  const image = new Image();
  image.src = selectionImage.value;
  await new Promise<void>((resolve, reject) => {
    image.onload = () => resolve();
    image.onerror = () => reject(new Error("Unable to load the screen capture"));
  });

  const bounds = surface.getBoundingClientRect();
  const scaleX = image.naturalWidth / bounds.width;
  const scaleY = image.naturalHeight / bounds.height;
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(chosen.width * scaleX);
  canvas.height = Math.round(chosen.height * scaleY);
  const context = canvas.getContext("2d");
  if (!context) {
    return;
  }

  context.drawImage(
    image,
    Math.round(chosen.left * scaleX),
    Math.round(chosen.top * scaleY),
    canvas.width,
    canvas.height,
    0,
    0,
    canvas.width,
    canvas.height,
  );
  const capturedImage = canvas.toDataURL("image/png");
  if (isCaptureWindow) {
    await emitTo("main", "capture-complete", capturedImage);
    localStorage.setItem("grabber.completedCapture", capturedImage);
    await currentWindow.hide();
    return;
  }

  showScreenshot(capturedImage);
  window.removeEventListener("keydown", onEscape);
  selectionImage.value = "";
  selection.value = null;
  isDragging.value = false;
  isSelecting.value = false;
}

function startCapture(capture: ScreenCapture) {
  selection.value = null;
  isDragging.value = false;
  selectionSurface.value = null;
  startPoint.value = { x: 0, y: 0 };
  selectionImage.value = capture.data_url;
  isSelecting.value = true;
  window.addEventListener("keydown", onEscape);
}

function loadPendingCapture() {
  const pending = localStorage.getItem("grabber.pendingCapture");
  if (!pending || !isCaptureWindow) {
    return;
  }

  const capture = JSON.parse(pending) as ScreenCapture;
  startCapture(capture);
}

function handleStorage(event: StorageEvent) {
  if (event.key === "grabber.completedCapture" && event.newValue && !isCaptureWindow) {
    showScreenshot(event.newValue);
    localStorage.removeItem("grabber.completedCapture");
  }

  if (event.key === "grabber.pendingCapture" && event.newValue && isCaptureWindow) {
    loadPendingCapture();
  }
}

onMounted(async () => {
  window.addEventListener("pointerdown", closeContextMenu);
  window.addEventListener("storage", handleStorage);
  if (isCaptureWindow) {
    unlistenCaptureStart = await listen<ScreenCapture>("capture-start", (event) => {
      startCapture(event.payload);
    });
  } else {
    unlistenCaptureComplete = await listen<string>("capture-complete", (event) => {
      showScreenshot(event.payload);
    });
  }
  loadPendingCapture();
});

onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", closeContextMenu);
  window.removeEventListener("keydown", onEscape);
  window.removeEventListener("storage", handleStorage);
  unlistenCaptureStart?.();
  unlistenCaptureComplete?.();
});

function zoomScreenshot(event: WheelEvent) {
  if (!screenshot.value) {
    return;
  }

  event.preventDefault();
  if (event.ctrlKey) {
    const viewer = screenshotViewer.value;
    const previousZoom = screenshotZoom.value;
    const step = event.deltaY < 0 ? 0.1 : -0.1;
    const nextZoom = Math.min(4, Math.max(0.25, previousZoom + step));
    if (viewer && nextZoom !== previousZoom) {
      const bounds = viewer.getBoundingClientRect();
      const cursorOffsetX = event.clientX - (bounds.left + bounds.width / 2);
      const cursorOffsetY = event.clientY - (bounds.top + bounds.height / 2);
      const zoomAdjustment = 1 - nextZoom / previousZoom;
      screenshotPanX.value +=
        zoomAdjustment * (cursorOffsetX - screenshotPanX.value);
      screenshotPanY.value +=
        zoomAdjustment * (cursorOffsetY - screenshotPanY.value);
    }
    screenshotZoom.value = nextZoom;
    return;
  }

  if (event.shiftKey) {
    screenshotPanX.value -= event.deltaY;
    return;
  }

  screenshotPanY.value -= event.deltaY;
}

function startScreenshotPan(event: PointerEvent) {
  if (event.button !== 1 || !event.isPrimary) {
    return;
  }

  const viewer = event.currentTarget as HTMLElement;
  isPanningScreenshot.value = true;
  screenshotPanStart.value = { x: event.clientX, y: event.clientY };
  screenshotPanOrigin.value = {
    x: screenshotPanX.value,
    y: screenshotPanY.value,
  };
  viewer.setPointerCapture(event.pointerId);
  event.preventDefault();
}

function moveScreenshotPan(event: PointerEvent) {
  if (!isPanningScreenshot.value) {
    return;
  }

  screenshotPanX.value =
    screenshotPanOrigin.value.x + event.clientX - screenshotPanStart.value.x;
  screenshotPanY.value =
    screenshotPanOrigin.value.y + event.clientY - screenshotPanStart.value.y;
}

function endScreenshotPan(event: PointerEvent) {
  if (!isPanningScreenshot.value) {
    return;
  }

  const viewer = event.currentTarget as HTMLElement;
  isPanningScreenshot.value = false;
  viewer.releasePointerCapture(event.pointerId);
}

function annotationPoint(event: PointerEvent) {
  const viewer = screenshotViewer.value;
  const image = screenshotImage.value;
  if (!viewer || !image) {
    return null;
  }

  const imageBounds = image.getBoundingClientRect();
  if (
    event.clientX < imageBounds.left ||
    event.clientX > imageBounds.right ||
    event.clientY < imageBounds.top ||
    event.clientY > imageBounds.bottom
  ) {
    return null;
  }

  return {
    x: (event.clientX - imageBounds.left) / imageBounds.width,
    y: (event.clientY - imageBounds.top) / imageBounds.height,
  };
}

function startAnnotation(event: PointerEvent) {
  if (
    (!isDrawingMode.value && !isShapeMode.value) ||
    event.button !== 0 ||
    !event.isPrimary
  ) {
    return;
  }

  const point = annotationPoint(event);
  if (!point) {
    return;
  }

  if (isShapeMode.value) {
    currentRectangle.value = {
      kind: selectedShape.value,
      start: point,
      end: point,
      color: brushColor.value,
      width: brushWidth.value,
      fill: fillShapes.value,
      order: annotationOrder.value++,
    };
  } else {
    currentAnnotation.value = [point];
  }
  (event.currentTarget as SVGElement).setPointerCapture(event.pointerId);
  event.preventDefault();
}

function moveAnnotation(event: PointerEvent) {
  if (isShapeMode.value && currentRectangle.value) {
    const point = annotationPoint(event);
    if (point) {
      currentRectangle.value = { ...currentRectangle.value, end: point };
    }
    return;
  }

  if (!currentAnnotation.value.length) {
    return;
  }

  const point = annotationPoint(event);
  if (point) {
    currentAnnotation.value = [...currentAnnotation.value, point];
  }
}

function endAnnotation(event: PointerEvent) {
  if (isShapeMode.value && currentRectangle.value) {
    annotationRectangles.value = [
      ...annotationRectangles.value,
      currentRectangle.value,
    ];
    currentRectangle.value = null;
    (event.currentTarget as SVGElement).releasePointerCapture(event.pointerId);
    return;
  }

  if (!currentAnnotation.value.length) {
    return;
  }

  annotationPaths.value = [
    ...annotationPaths.value,
    {
      kind: "stroke",
      points: currentAnnotation.value,
      color: brushColor.value,
      width: brushWidth.value,
      order: annotationOrder.value++,
    },
  ];
  currentAnnotation.value = [];
  (event.currentTarget as SVGElement).releasePointerCapture(event.pointerId);
}

function annotationPoints(path: AnnotationPoint[]) {
  return path.map((point) => `${point.x * 100},${point.y * 100}`).join(" ");
}

function rectangleGeometry(rectangle: AnnotationRectangle) {
  return {
    x: Math.min(rectangle.start.x, rectangle.end.x) * 100,
    y: Math.min(rectangle.start.y, rectangle.end.y) * 100,
    width: Math.abs(rectangle.end.x - rectangle.start.x) * 100,
    height: Math.abs(rectangle.end.y - rectangle.start.y) * 100,
  };
}

function annotationLayerStyle() {
  const viewer = screenshotViewer.value;
  const image = screenshotImage.value;
  if (!viewer || !image) {
    return {};
  }

  const viewerBounds = viewer.getBoundingClientRect();
  const imageBounds = image.getBoundingClientRect();
  return {
    left: `${imageBounds.left - viewerBounds.left}px`,
    top: `${imageBounds.top - viewerBounds.top}px`,
    width: `${imageBounds.width}px`,
    height: `${imageBounds.height}px`,
  };
}

async function copyAnnotatedScreenshot() {
  if (!screenshot.value) {
    return;
  }

  const image = new Image();
  image.src = screenshot.value;
  await new Promise<void>((resolve, reject) => {
    image.onload = () => resolve();
    image.onerror = () => reject(new Error("Unable to prepare screenshot for copying"));
  });

  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth;
  canvas.height = image.naturalHeight;
  const context = canvas.getContext("2d");
  if (!context) {
    return;
  }

  context.drawImage(image, 0, 0);
  context.lineCap = "round";
  context.lineJoin = "round";

  const strokes = [
    ...annotationPaths.value,
    {
      points: currentAnnotation.value,
      color: brushColor.value,
      width: brushWidth.value,
    },
  ];
  for (const stroke of strokes) {
    const path = stroke.points;
    if (!path.length) {
      continue;
    }

    context.strokeStyle = stroke.color;
    context.lineWidth = Math.max(4, (image.naturalWidth / 300) * (stroke.width / 4));
    context.beginPath();
    context.moveTo(path[0].x * canvas.width, path[0].y * canvas.height);
    for (const point of path.slice(1)) {
      context.lineTo(point.x * canvas.width, point.y * canvas.height);
    }
    context.stroke();
  }

  for (const rectangle of [
    ...annotationRectangles.value,
    ...(currentRectangle.value ? [currentRectangle.value] : []),
  ]) {
    const geometry = rectangleGeometry(rectangle);
    context.strokeStyle = rectangle.color;
    context.lineWidth = Math.max(4, (image.naturalWidth / 300) * (rectangle.width / 4));
    const startX = (rectangle.start.x / 100) * canvas.width;
    const startY = (rectangle.start.y / 100) * canvas.height;
    const endX = (rectangle.end.x / 100) * canvas.width;
    const endY = (rectangle.end.y / 100) * canvas.height;
    if (rectangle.kind === "line" || rectangle.kind === "arrow") {
      context.beginPath();
      context.moveTo(rectangle.start.x * canvas.width, rectangle.start.y * canvas.height);
      context.lineTo(rectangle.end.x * canvas.width, rectangle.end.y * canvas.height);
      context.stroke();
      if (rectangle.kind === "arrow") {
        const angle = Math.atan2(endY - startY, endX - startX);
        const headLength = Math.max(3, context.lineWidth * 1.2);
        context.beginPath();
        context.moveTo(endX, endY);
        context.lineTo(
          endX - headLength * Math.cos(angle - Math.PI / 6),
          endY - headLength * Math.sin(angle - Math.PI / 6),
        );
        context.moveTo(endX, endY);
        context.lineTo(
          endX - headLength * Math.cos(angle + Math.PI / 6),
          endY - headLength * Math.sin(angle + Math.PI / 6),
        );
        context.stroke();
      }
    } else if (rectangle.kind === "oval") {
      context.beginPath();
      context.ellipse(
        (startX + endX) / 2,
        (startY + endY) / 2,
        Math.abs(endX - startX) / 2,
        Math.abs(endY - startY) / 2,
        0,
        0,
        Math.PI * 2,
      );
      if (rectangle.fill) {
        context.fillStyle = rectangle.color;
        context.fill();
      }
      context.stroke();
    } else if (rectangle.kind === "triangle") {
      context.beginPath();
      context.moveTo((geometry.x + geometry.width / 2) * canvas.width / 100, geometry.y * canvas.height / 100);
      context.lineTo((geometry.x + geometry.width) * canvas.width / 100, (geometry.y + geometry.height) * canvas.height / 100);
      context.lineTo(geometry.x * canvas.width / 100, (geometry.y + geometry.height) * canvas.height / 100);
      context.closePath();
      if (rectangle.fill) {
        context.fillStyle = rectangle.color;
        context.fill();
      }
      context.stroke();
    } else {
      if (rectangle.fill) {
        context.fillStyle = rectangle.color;
        context.fillRect(
          (geometry.x / 100) * canvas.width,
          (geometry.y / 100) * canvas.height,
          (geometry.width / 100) * canvas.width,
          (geometry.height / 100) * canvas.height,
        );
      }
      context.strokeRect(
        (geometry.x / 100) * canvas.width,
        (geometry.y / 100) * canvas.height,
        (geometry.width / 100) * canvas.width,
        (geometry.height / 100) * canvas.height,
      );
    }
  }

  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (blob && navigator.clipboard && typeof ClipboardItem !== "undefined") {
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
  }
  contextMenu.value.visible = false;
}

function openContextMenu(event: MouseEvent) {
  contextMenu.value = {
    visible: true,
    x: event.clientX,
    y: event.clientY,
  };
}

function closeContextMenu() {
  contextMenu.value.visible = false;
}

function toggleBrush() {
  if (isDrawingMode.value) {
    brushMenuOpen.value = !brushMenuOpen.value;
  } else {
    isShapeMode.value = false;
    shapeMenuOpen.value = false;
    isDrawingMode.value = true;
  }
}

function selectShape(kind: AnnotationRectangle["kind"]) {
  isShapeMode.value = true;
  isDrawingMode.value = false;
  brushMenuOpen.value = false;
  currentRectangle.value = currentRectangle.value
    ? { ...currentRectangle.value, kind }
    : null;
  selectedShape.value = kind;
}

function toggleShapesMenu() {
  isShapeMode.value = true;
  isDrawingMode.value = false;
  brushMenuOpen.value = false;
  shapeMenuOpen.value = !shapeMenuOpen.value;
}
</script>

<template>
  <v-app>
    <v-navigation-drawer permanent width="64" class="side-panel">
      <v-menu
        v-model="brushMenuOpen"
        location="end"
        :close-on-content-click="false"
        :open-on-click="false"
      >
        <template #activator="{ props }">
          <v-btn
            v-bind="props"
            class="annotation-button"
            :color="isDrawingMode ? 'primary' : undefined"
            icon="mdi-brush"
            variant="text"
            aria-label="Brush"
            title="Brush"
            :disabled="!screenshot"
            @click="toggleBrush"
          />
        </template>
        <v-card class="brush-menu pa-3" width="220">
          <v-text-field
            v-model="brushColor"
            label="Color"
            type="color"
            variant="outlined"
            density="compact"
            hide-details
          />
          <div class="brush-width-control">
            <v-slider
              v-model="brushWidth"
              label="Width"
              min="2"
              max="16"
              step="1"
              thumb-label
              hide-details
            />
            <span class="brush-preview-box" aria-label="Brush size preview">
              <span
                class="brush-preview-dot"
                :style="{
                  width: `${brushWidth}px`,
                  height: `${brushWidth}px`,
                  backgroundColor: brushColor,
                }"
              />
            </span>
          </div>
        </v-card>
      </v-menu>
      <v-menu
        v-model="shapeMenuOpen"
        location="end"
        :close-on-content-click="false"
        :open-on-click="false"
      >
        <template #activator="{ props }">
          <v-btn
            v-bind="props"
            class="annotation-button"
            :color="isShapeMode ? 'primary' : undefined"
            icon="mdi-shape-outline"
            variant="text"
            aria-label="Shapes"
            title="Shapes"
            :disabled="!screenshot"
            @click="toggleShapesMenu"
          />
        </template>
        <v-card width="180">
          <v-list density="compact">
            <v-list-item
              prepend-icon="mdi-rectangle-outline"
              title="Rectangle"
              :active="selectedShape === 'rectangle'"
              active-color="primary"
              @click="selectShape('rectangle')"
            />
            <v-list-item
              prepend-icon="mdi-ellipse-outline"
              title="Oval"
              :active="selectedShape === 'oval'"
              active-color="primary"
              @click="selectShape('oval')"
            />
            <v-list-item
              prepend-icon="mdi-vector-line"
              title="Line"
              :active="selectedShape === 'line'"
              active-color="primary"
              @click="selectShape('line')"
            />
            <v-list-item
              prepend-icon="mdi-arrow-top-right"
              title="Arrow"
              :active="selectedShape === 'arrow'"
              active-color="primary"
              @click="selectShape('arrow')"
            />
            <v-list-item
              prepend-icon="mdi-triangle-outline"
              title="Triangle"
              :active="selectedShape === 'triangle'"
              active-color="primary"
              @click="selectShape('triangle')"
            />
          </v-list>
          <v-checkbox
            v-model="fillShapes"
            label="Fill"
            density="compact"
            hide-details
            class="px-3"
            :disabled="selectedShape !== 'rectangle' && selectedShape !== 'oval' && selectedShape !== 'triangle'"
          />
        </v-card>
      </v-menu>
    </v-navigation-drawer>

    <v-app-bar flat height="64" class="top-bar">
      <v-spacer />
      <v-btn
        icon="mdi-camera-outline"
        variant="text"
        aria-label="Take screenshot"
        title="Take screenshot"
        @click="beginSelection"
      />
      <v-spacer />
    </v-app-bar>

    <v-main class="main-content" @wheel.prevent.stop="zoomScreenshot">
      <div
        v-if="screenshot"
        ref="screenshotViewer"
        class="screenshot-viewer"
        @pointerdown="startScreenshotPan"
        @pointermove="moveScreenshotPan"
        @pointerup="endScreenshotPan"
        @pointercancel="endScreenshotPan"
        @contextmenu.prevent="openContextMenu"
      >
        <img
          ref="screenshotImage"
          :src="screenshot"
          class="screenshot-image"
          :style="{
            transform: `translate(${screenshotPanX}px, ${screenshotPanY}px) scale(${screenshotZoom})`,
          }"
          alt="Selected screenshot"
        />
        <svg
          v-if="isDrawingMode || isShapeMode"
          class="annotation-layer"
          :style="annotationLayerStyle()"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          @pointerdown.stop="startAnnotation"
          @pointermove.stop="moveAnnotation"
          @pointerup.stop="endAnnotation"
          @pointercancel.stop="endAnnotation"
        >
          <defs>
            <marker
              id="annotation-arrowhead"
              markerWidth="3"
              markerHeight="3"
              refX="2.5"
              refY="1.5"
              orient="auto"
              markerUnits="userSpaceOnUse"
            >
              <path d="M 0 0 L 3 1.5 L 0 3 z" fill="context-stroke" />
            </marker>
          </defs>
          <g v-for="(annotation, index) in orderedAnnotations" :key="index">
            <polyline
              v-if="annotation.kind === 'stroke'"
              :points="annotationPoints(annotation.points)"
              fill="none"
              :stroke="annotation.color"
              stroke-linecap="round"
              stroke-linejoin="round"
              :stroke-width="annotation.width"
              vector-effect="non-scaling-stroke"
            />
            <rect
              v-else-if="annotation.kind === 'rectangle'"
              v-bind="rectangleGeometry(annotation)"
              :fill="annotation.fill ? annotation.color : 'none'"
              :stroke="annotation.color"
              :stroke-width="annotation.width"
              vector-effect="non-scaling-stroke"
            />
            <ellipse
              v-else-if="annotation.kind === 'oval'"
              :cx="(annotation.start.x + annotation.end.x) * 50"
              :cy="(annotation.start.y + annotation.end.y) * 50"
              :rx="Math.abs(annotation.end.x - annotation.start.x) * 50"
              :ry="Math.abs(annotation.end.y - annotation.start.y) * 50"
              :fill="annotation.fill ? annotation.color : 'none'"
              :stroke="annotation.color"
              :stroke-width="annotation.width"
              vector-effect="non-scaling-stroke"
            />
            <polygon
              v-else-if="annotation.kind === 'triangle'"
              :points="`${(annotation.start.x + annotation.end.x) * 50},${Math.min(annotation.start.y, annotation.end.y) * 100} ${Math.max(annotation.start.x, annotation.end.x) * 100},${Math.max(annotation.start.y, annotation.end.y) * 100} ${Math.min(annotation.start.x, annotation.end.x) * 100},${Math.max(annotation.start.y, annotation.end.y) * 100}`"
              :fill="annotation.fill ? annotation.color : 'none'"
              :stroke="annotation.color"
              :stroke-width="annotation.width"
              vector-effect="non-scaling-stroke"
            />
            <line
              v-else
              :x1="annotation.start.x * 100"
              :y1="annotation.start.y * 100"
              :x2="annotation.end.x * 100"
              :y2="annotation.end.y * 100"
              :stroke="annotation.color"
              :stroke-width="annotation.width"
              stroke-linecap="round"
              :marker-end="annotation.kind === 'arrow' ? 'url(#annotation-arrowhead)' : undefined"
              vector-effect="non-scaling-stroke"
            />
          </g>
          <polyline
            v-if="currentAnnotation.length"
            :points="annotationPoints(currentAnnotation)"
            fill="none"
            :stroke="brushColor"
            stroke-linecap="round"
            stroke-linejoin="round"
            :stroke-width="brushWidth"
            vector-effect="non-scaling-stroke"
          />
          <g v-if="currentRectangle">
            <rect
              v-if="currentRectangle.kind === 'rectangle'"
              v-bind="rectangleGeometry(currentRectangle)"
              :fill="currentRectangle.fill ? currentRectangle.color : 'none'"
              :stroke="currentRectangle.color"
              :stroke-width="currentRectangle.width"
              vector-effect="non-scaling-stroke"
            />
            <ellipse
              v-else-if="currentRectangle.kind === 'oval'"
              :cx="(currentRectangle.start.x + currentRectangle.end.x) * 50"
              :cy="(currentRectangle.start.y + currentRectangle.end.y) * 50"
              :rx="Math.abs(currentRectangle.end.x - currentRectangle.start.x) * 50"
              :ry="Math.abs(currentRectangle.end.y - currentRectangle.start.y) * 50"
              :fill="currentRectangle.fill ? currentRectangle.color : 'none'"
              :stroke="currentRectangle.color"
              :stroke-width="currentRectangle.width"
              vector-effect="non-scaling-stroke"
            />
            <polygon
              v-else-if="currentRectangle.kind === 'triangle'"
              :points="`${(currentRectangle.start.x + currentRectangle.end.x) * 50},${Math.min(currentRectangle.start.y, currentRectangle.end.y) * 100} ${Math.max(currentRectangle.start.x, currentRectangle.end.x) * 100},${Math.max(currentRectangle.start.y, currentRectangle.end.y) * 100} ${Math.min(currentRectangle.start.x, currentRectangle.end.x) * 100},${Math.max(currentRectangle.start.y, currentRectangle.end.y) * 100}`"
              :fill="currentRectangle.fill ? currentRectangle.color : 'none'"
              :stroke="currentRectangle.color"
              :stroke-width="currentRectangle.width"
              vector-effect="non-scaling-stroke"
            />
            <line
              v-else
              :x1="currentRectangle.start.x * 100"
              :y1="currentRectangle.start.y * 100"
              :x2="currentRectangle.end.x * 100"
              :y2="currentRectangle.end.y * 100"
              :stroke="currentRectangle.color"
              :stroke-width="currentRectangle.width"
              stroke-linecap="round"
              :marker-end="currentRectangle.kind === 'arrow' ? 'url(#annotation-arrowhead)' : undefined"
              vector-effect="non-scaling-stroke"
            />
          </g>
        </svg>
      </div>
    </v-main>

    <div
      v-if="contextMenu.visible"
      class="context-menu"
      :style="{ left: `${contextMenu.x}px`, top: `${contextMenu.y}px` }"
      @pointerdown.stop
    >
      <v-btn variant="text" prepend-icon="mdi-content-copy" @click="copyAnnotatedScreenshot">
        Copy image
      </v-btn>
    </div>

    <div
      v-if="isSelecting"
      ref="selectionSurface"
      class="selection-surface"
      :class="{ 'has-selection': selection }"
      @pointerdown="startDrag"
      @pointermove="updateSelection"
      @pointerup="completeDrag"
    >
      <img :src="selectionImage" class="screen-image" alt="Screen capture" draggable="false" />
      <div v-if="selection" class="selection-box" :style="selectionStyle" />
    </div>
  </v-app>
</template>

<style>
body {
  margin: 0;
}

.main-content {
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden !important;
  overscroll-behavior: none;
}

.top-bar {
  left: 0 !important;
  width: 100% !important;
  border-bottom: 1px solid rgba(0, 0, 0, 0.12);
}

.side-panel {
  top: 64px !important;
  height: calc(100% - 64px) !important;
}

.context-menu {
  position: fixed;
  z-index: 3000;
  padding: 4px;
  border: 1px solid rgba(0, 0, 0, 0.12);
  border-radius: 4px;
  background: white;
  box-shadow: 0 3px 12px rgba(0, 0, 0, 0.2);
}

.screenshot-viewer {
  position: relative;
  display: flex;
  width: 100%;
  height: 100%;
  align-items: center;
  justify-content: center;
  overflow: hidden;
}

.screenshot-image {
  display: block;
  width: auto;
  height: auto;
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  transform-origin: center;
}

.annotation-button {
  margin: 8px;
}

.brush-width-control {
  display: flex;
  align-items: center;
  gap: 10px;
}

.brush-width-control .v-slider {
  flex: 1;
}

.brush-preview-box {
  display: flex;
  width: 24px;
  height: 24px;
  flex: 0 0 24px;
  align-items: center;
  justify-content: center;
}

.brush-preview-dot {
  display: block;
  border-radius: 50%;
}

.annotation-layer {
  position: absolute;
  cursor: crosshair;
  pointer-events: auto;
  transform-origin: center;
}

.selection-surface {
  position: fixed;
  z-index: 2000;
  inset: 0;
  cursor: crosshair;
  user-select: none;
  touch-action: none;
  overflow: hidden;
}

.selection-surface::after {
  position: absolute;
  inset: 0;
  background: rgb(0 0 0 / 22%);
  content: "";
  pointer-events: none;
}

.selection-surface.has-selection::after {
  display: none;
}

.screen-image {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: fill;
  pointer-events: none;
}

.selection-box {
  position: absolute;
  border: 2px solid #ffffff;
  box-shadow: 0 0 0 9999px rgb(0 0 0 / 22%);
  pointer-events: none;
}
</style>

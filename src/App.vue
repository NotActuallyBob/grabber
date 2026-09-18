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

const currentWindow = getCurrentWindow();
const isCaptureWindow = currentWindow.label === "capture";
const selectionImage = ref("");
const screenshot = ref("");
const screenshotZoom = ref(1);
const screenshotPanX = ref(0);
const screenshotPanY = ref(0);
const isDrawingMode = ref(false);
const annotationPaths = ref<AnnotationPoint[][]>([]);
const currentAnnotation = ref<AnnotationPoint[]>([]);
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
  currentAnnotation.value = [];
  isDrawingMode.value = false;
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
  if (!isDrawingMode.value || event.button !== 0 || !event.isPrimary) {
    return;
  }

  const point = annotationPoint(event);
  if (!point) {
    return;
  }

  currentAnnotation.value = [point];
  (event.currentTarget as SVGElement).setPointerCapture(event.pointerId);
  event.preventDefault();
}

function moveAnnotation(event: PointerEvent) {
  if (!currentAnnotation.value.length) {
    return;
  }

  const point = annotationPoint(event);
  if (point) {
    currentAnnotation.value = [...currentAnnotation.value, point];
  }
}

function endAnnotation(event: PointerEvent) {
  if (!currentAnnotation.value.length) {
    return;
  }

  annotationPaths.value = [...annotationPaths.value, currentAnnotation.value];
  currentAnnotation.value = [];
  (event.currentTarget as SVGElement).releasePointerCapture(event.pointerId);
}

function annotationPoints(path: AnnotationPoint[]) {
  return path.map((point) => `${point.x * 100},${point.y * 100}`).join(" ");
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
</script>

<template>
  <v-app>
    <v-navigation-drawer permanent width="64" class="side-panel">
      <v-btn
        class="annotation-button"
        :color="isDrawingMode ? 'primary' : undefined"
        icon="mdi-pen"
        variant="text"
        aria-label="Annotate screenshot"
        title="Annotate screenshot"
        :disabled="!screenshot"
        @click="isDrawingMode = !isDrawingMode"
      />
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
          v-if="isDrawingMode"
          class="annotation-layer"
          :style="annotationLayerStyle()"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          @pointerdown.stop="startAnnotation"
          @pointermove.stop="moveAnnotation"
          @pointerup.stop="endAnnotation"
          @pointercancel.stop="endAnnotation"
        >
          <polyline
            v-for="(path, index) in [...annotationPaths, currentAnnotation]"
            :key="index"
            :points="annotationPoints(path)"
            fill="none"
            stroke="#e53935"
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="0.7"
          />
        </svg>
      </div>
    </v-main>

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

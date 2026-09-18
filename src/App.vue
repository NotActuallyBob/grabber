<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/dpi";
import { getCurrentWindow } from "@tauri-apps/api/window";

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

const currentWindow = getCurrentWindow();
const selectionImage = ref("");
const screenshot = ref("");
const isSelecting = ref(false);
const isDragging = ref(false);
const selection = ref<Selection | null>(null);
const selectionSurface = ref<HTMLElement | null>(null);
const startPoint = ref({ x: 0, y: 0 });
const originalPosition = ref<PhysicalPosition | null>(null);
const originalSize = ref<PhysicalSize | null>(null);
const originalResizable = ref<boolean | null>(null);

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

async function restoreWindow() {
  await currentWindow.setDecorations(true);
  await currentWindow.setShadow(true);
  await currentWindow.setSkipTaskbar(false);
  await currentWindow.setResizable(originalResizable.value ?? true);
  if (originalPosition.value && originalSize.value) {
    await currentWindow.setPosition(originalPosition.value);
    await currentWindow.setSize(originalSize.value);
  }
  await currentWindow.setAlwaysOnTop(false);
  originalPosition.value = null;
  originalSize.value = null;
  originalResizable.value = null;
}

async function cancelSelection() {
  selectionImage.value = "";
  selection.value = null;
  isDragging.value = false;
  isSelecting.value = false;
  await restoreWindow();
}

function onEscape(event: KeyboardEvent) {
  if (event.key === "Escape" && isSelecting.value) {
    void cancelSelection();
  }
}

async function beginSelection() {
  try {
    const capture = await invoke<ScreenCapture>("capture_screen");
    originalPosition.value = await currentWindow.outerPosition();
    originalSize.value = await currentWindow.outerSize();
    originalResizable.value = await currentWindow.isResizable();
    selectionImage.value = capture.data_url;
    await currentWindow.setAlwaysOnTop(true);
    await currentWindow.setSkipTaskbar(true);
    await currentWindow.setResizable(false);
    await currentWindow.setDecorations(false);
    await currentWindow.setShadow(false);
    await currentWindow.setPosition(new PhysicalPosition(capture.x, capture.y));
    await currentWindow.setSize(new PhysicalSize(capture.width, capture.height));
    await nextTick();
    isSelecting.value = true;
    window.addEventListener("keydown", onEscape);
  } catch (error) {
    console.error("Unable to start screen capture", error);
    selectionImage.value = "";
    await restoreWindow();
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
  screenshot.value = canvas.toDataURL("image/png");
  window.removeEventListener("keydown", onEscape);
  selectionImage.value = "";
  selection.value = null;
  isDragging.value = false;
  isSelecting.value = false;
  await restoreWindow();
}

onBeforeUnmount(() => window.removeEventListener("keydown", onEscape));
</script>

<template>
  <v-app>
    <v-app-bar v-if="!isSelecting" flat>
      <v-spacer />
      <v-btn
        icon="mdi-camera-outline"
        variant="text"
        aria-label="Take screenshot"
        title="Take screenshot"
        @click="beginSelection"
      />
    </v-app-bar>

    <v-main class="main-content">
      <v-img v-if="screenshot" :src="screenshot" contain max-height="calc(100vh - 64px)" />
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

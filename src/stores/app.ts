import { defineStore } from "pinia";
import { ref } from "vue";

export const useAppStore = defineStore("app", () => {
  const url = ref("");
  const isQueued = ref(false);

  function queueDownload() {
    if (url.value) {
      isQueued.value = true;
    }
  }

  function clearQueue() {
    isQueued.value = false;
  }

  function refresh() {
    clearQueue();
    url.value = "";
  }

  return { url, isQueued, queueDownload, clearQueue, refresh };
});
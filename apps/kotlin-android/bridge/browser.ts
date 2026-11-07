export {
  start,
  stop,
  command,
  pictureInPicture,
} from "../../mobile/src/features/browser/preview-stream.browser.ts";
import {
  uploadPreviewStreamFiles,
  type PreviewStreamFileChooser,
} from "../../../packages/client-runtime/src/preview/serverBrowserStream.ts";

/** Let Android's system picker provide files to the remote page's pending upload. */
export function selectFiles(chooser: PreviewStreamFileChooser) {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = chooser.accept;
  input.multiple = chooser.multiple;
  input.style.display = "none";
  const finish = async (files: File[]) => {
    try {
      await uploadPreviewStreamFiles(chooser, files);
    } catch (error) {
      window.ReactNativeWebView.postMessage(
        JSON.stringify({ type: "uploadError", detail: String(error) }),
      );
    } finally {
      input.remove();
    }
  };
  input.onchange = () => void finish(Array.from(input.files ?? []));
  input.oncancel = () => void finish([]);
  document.body.append(input);
  input.click();
}

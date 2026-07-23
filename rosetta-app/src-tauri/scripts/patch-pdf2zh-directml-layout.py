#!/usr/bin/env python3
"""Add bounded DirectML layout batching to Rosetta's pdf2zh runtime."""

from __future__ import annotations

from pathlib import Path

import pdf2zh


def patch_doclayout(root: Path) -> bool:
    target = root / "doclayout.py"
    text = target.read_text(encoding="utf-8")
    if "def predict_batch(" in text and "def preferred_batch_size(" in text:
        return False

    import_anchor = "import abc\n"
    if "from collections import defaultdict\n" not in text:
        if import_anchor not in text:
            raise SystemExit(f"::error::could not find doclayout import anchor in {target}")
        text = text.replace(
            import_anchor,
            import_anchor + "from collections import defaultdict\n",
            1,
        )

    old_session = '''        sess_options = onnxruntime.SessionOptions()
        sess_options.graph_optimization_level = (
            onnxruntime.GraphOptimizationLevel.ORT_ENABLE_ALL
        )

        if _preferred_backend and _preferred_backend in _BACKEND_PROVIDERS:
            providers = _BACKEND_PROVIDERS[_preferred_backend]
        else:
            providers = onnxruntime.get_available_providers()

        # Providers like CoreML generate compiled nodes that cannot be
        # serialized, so only cache the optimized graph for CPU-only.
        compiled_providers = {"CoreMLExecutionProvider", "TensorrtExecutionProvider"}
        can_cache = not compiled_providers.intersection(providers)
        if can_cache:
            optimized_path = model_path + ".optimized"
            if os.path.exists(optimized_path):
                model_path = optimized_path
            else:
                sess_options.optimized_model_filepath = optimized_path

        self.model = onnxruntime.InferenceSession(
            model_path, sess_options, providers=providers
        )
        logger.info("ONNX Runtime providers: %s", self.model.get_providers())
'''
    new_session = '''        if _preferred_backend and _preferred_backend in _BACKEND_PROVIDERS:
            providers = _BACKEND_PROVIDERS[_preferred_backend]
        else:
            providers = onnxruntime.get_available_providers()
        self.model = self._create_session(providers)
        self._uses_directml = "DmlExecutionProvider" in self.model.get_providers()
        logger.info("ONNX Runtime providers: %s", self.model.get_providers())

    def _create_session(self, providers):
        sess_options = onnxruntime.SessionOptions()
        sess_options.graph_optimization_level = (
            onnxruntime.GraphOptimizationLevel.ORT_ENABLE_ALL
        )
        if "DmlExecutionProvider" in providers:
            sess_options.enable_mem_pattern = False
            sess_options.execution_mode = onnxruntime.ExecutionMode.ORT_SEQUENTIAL

        model_path = self.model_path
        compiled_providers = {
            "CoreMLExecutionProvider",
            "DmlExecutionProvider",
            "TensorrtExecutionProvider",
        }
        can_cache = not compiled_providers.intersection(providers)
        if can_cache:
            optimized_path = model_path + ".optimized"
            if os.path.exists(optimized_path):
                model_path = optimized_path
            else:
                sess_options.optimized_model_filepath = optimized_path

        try:
            return onnxruntime.InferenceSession(
                model_path, sess_options, providers=providers
            )
        except Exception:
            if "DmlExecutionProvider" not in providers:
                raise
            logger.exception(
                "DirectML layout initialization failed; falling back to CPU"
            )
            return self._create_session(["CPUExecutionProvider"])
'''
    if old_session not in text:
        raise SystemExit(f"::error::could not find doclayout session block in {target}")
    text = text.replace(old_session, new_session, 1)

    old_predict = '''    def predict(self, image, imgsz=1024, **kwargs):
        # Preprocess input image
        orig_h, orig_w = image.shape[:2]
        pix = self.resize_and_pad_image(image, new_shape=imgsz)
        pix = np.transpose(pix, (2, 0, 1))  # CHW
        pix = np.expand_dims(pix, axis=0)  # BCHW
        pix = pix.astype(np.float32) / 255.0  # Normalize to [0, 1]
        new_h, new_w = pix.shape[2:]

        # Run inference
        preds = self.model.run(None, {"images": pix})[0]

        # Postprocess predictions
        preds = preds[preds[..., 4] > 0.25]
        preds[..., :4] = self.scale_boxes(
            (new_h, new_w), preds[..., :4], (orig_h, orig_w)
        )
        return [YoloResult(boxes=preds, names=self._names)]
'''
    new_predict = '''    @property
    def preferred_batch_size(self):
        return 5 if self._uses_directml else 1

    def prepare_input(self, image, imgsz):
        orig_h, orig_w = image.shape[:2]
        pix = self.resize_and_pad_image(image, new_shape=imgsz)
        pix = np.transpose(pix, (2, 0, 1))  # CHW
        pix = pix.astype(np.float32) / 255.0  # Normalize to [0, 1]
        return pix, (orig_h, orig_w)

    def run_inference(self, pix):
        try:
            return self.model.run(None, {"images": pix})[0]
        except Exception:
            if not self._uses_directml:
                raise
            logger.exception("DirectML layout inference failed; falling back to CPU")
            self.model = self._create_session(["CPUExecutionProvider"])
            self._uses_directml = False
            return self.model.run(None, {"images": pix})[0]

    def postprocess_prediction(self, preds, input_shape, original_shape):
        new_h, new_w = input_shape
        preds = preds[preds[..., 4] > 0.25]
        preds[..., :4] = self.scale_boxes(
            (new_h, new_w), preds[..., :4], original_shape
        )
        return YoloResult(boxes=preds, names=self._names)

    def predict(self, image, imgsz=1024, **kwargs):
        pix, original_shape = self.prepare_input(image, imgsz)
        batch = np.expand_dims(pix, axis=0)
        preds = self.run_inference(batch)[0]
        return [
            self.postprocess_prediction(preds, batch.shape[2:], original_shape)
        ]

    def predict_batch(self, images, imgsz=1024, batch_size=5, **kwargs):
        if not images:
            return []

        prepared = [self.prepare_input(image, imgsz) for image in images]
        grouped = defaultdict(list)
        for index, (pix, original_shape) in enumerate(prepared):
            grouped[pix.shape].append((index, pix, original_shape))

        results = [None] * len(images)
        for items in grouped.values():
            for start in range(0, len(items), batch_size):
                batch_items = items[start : start + batch_size]
                batch = np.stack([item[1] for item in batch_items])
                predictions = self.run_inference(batch)
                for (index, _pix, original_shape), preds in zip(
                    batch_items, predictions
                ):
                    results[index] = self.postprocess_prediction(
                        preds, batch.shape[2:], original_shape
                    )
        return results
'''
    if old_predict not in text:
        raise SystemExit(f"::error::could not find doclayout predict block in {target}")
    target.write_text(text.replace(old_predict, new_predict, 1), encoding="utf-8")
    print(f"[pdf2zh-pack] enabled DirectML layout inference in {target}")
    return True


def patch_engine(root: Path) -> bool:
    target = root / "rosetta_engine.py"
    text = target.read_text(encoding="utf-8")
    if "def build_layout_masks(" in text and "def layout_batch_size(" in text:
        return False

    old_loop = '''        for prepared_page_index, (page, page_number) in enumerate(zip(pdf_pages, selected_pages)):
            page_index = prepared_page_index
            page.pageno = page_index
            if persistent_layout is None:
                layout[page_index] = build_layout_mask(doc, page_index, model, options)
            else:
                layout[page_index] = persistent_layout[page_index]
            cache = collect_page_units(
                page=page,
                page_index=page_index,
                page_number=page_number,
                layout=layout,
                translator=collector,
                rsrcmgr=rsrcmgr,
                lang_in=langIn,
                lang_out=langOut,
                thread=int(options.get("thread") or 1),
                noto_name=noto_name,
                noto=noto,
            )
            page_caches[page_number] = cache
'''
    new_loop = '''        batch_size = layout_batch_size(model, options)
        page_pairs = [
            (page, page_number, prepared_page_index)
            for prepared_page_index, (page, page_number) in enumerate(
                zip(pdf_pages, selected_pages)
            )
        ]
        for start in range(0, len(page_pairs), batch_size):
            batch = page_pairs[start : start + batch_size]
            page_indices = [page_index for _page, _page_number, page_index in batch]
            if persistent_layout is None:
                layout.update(build_layout_masks(doc, page_indices, model, options))
            else:
                layout.update(
                    {page_index: persistent_layout[page_index] for page_index in page_indices}
                )
            for page, page_number, page_index in batch:
                page.pageno = page_index
                cache = collect_page_units(
                    page=page,
                    page_index=page_index,
                    page_number=page_number,
                    layout=layout,
                    translator=collector,
                    rsrcmgr=rsrcmgr,
                    lang_in=langIn,
                    lang_out=langOut,
                    thread=int(options.get("thread") or 1),
                    noto_name=noto_name,
                    noto=noto,
                )
                page_caches[page_number] = cache
'''
    if old_loop not in text:
        raise SystemExit(f"::error::could not find prepared layout loop in {target}")
    text = text.replace(old_loop, new_loop, 1)

    old_layout = '''def build_layout_mask(doc, page_index: int, model, options: dict[str, Any]):
    pix = doc[page_index].get_pixmap()
    image = np.frombuffer(pix.samples, np.uint8).reshape(pix.height, pix.width, 3)[:, :, ::-1]
    imgsz = layout_imgsz_for_pix(pix, options)
    device_name = layout_device_name(model)
    page_layout = model.predict(image, imgsz=imgsz, device=device_name, verbose=False)[0]
    box = np.ones((pix.height, pix.width))
'''
    new_layout = '''def build_layout_mask(doc, page_index: int, model, options: dict[str, Any]):
    pix = doc[page_index].get_pixmap()
    image = np.frombuffer(pix.samples, np.uint8).reshape(pix.height, pix.width, 3)[:, :, ::-1]
    imgsz = layout_imgsz_for_pix(pix, options)
    device_name = layout_device_name(model)
    page_layout = model.predict(image, imgsz=imgsz, device=device_name, verbose=False)[0]
    return build_layout_mask_from_prediction(pix, page_layout)


def build_layout_masks(doc, page_indices: list[int], model, options: dict[str, Any]):
    batch_size = layout_batch_size(model, options)
    if batch_size <= 1 or not callable(getattr(model, "predict_batch", None)):
        return {
            page_index: build_layout_mask(doc, page_index, model, options)
            for page_index in page_indices
        }

    prepared = []
    for page_index in page_indices:
        pix = doc[page_index].get_pixmap()
        image = np.frombuffer(pix.samples, np.uint8).reshape(
            pix.height, pix.width, 3
        )[:, :, ::-1]
        prepared.append((page_index, pix, image, layout_imgsz_for_pix(pix, options)))

    predictions = {}
    groups = {}
    for page_index, pix, image, imgsz in prepared:
        groups.setdefault(imgsz, []).append((page_index, pix, image))
    for imgsz, group in groups.items():
        for start in range(0, len(group), batch_size):
            batch = group[start : start + batch_size]
            page_layouts = model.predict_batch(
                [item[2] for item in batch],
                imgsz=imgsz,
                batch_size=batch_size,
                device=layout_device_name(model),
                verbose=False,
            )
            for (page_index, pix, _image), page_layout in zip(batch, page_layouts):
                predictions[page_index] = build_layout_mask_from_prediction(
                    pix, page_layout
                )
    return predictions


def build_layout_mask_from_prediction(pix, page_layout):
    box = np.ones((pix.height, pix.width))
'''
    if old_layout not in text:
        raise SystemExit(f"::error::could not find layout mask anchor in {target}")
    text = text.replace(old_layout, new_layout, 1)

    helper_anchor = "\n\ndef get_layout_model(model_path: str):\n"
    helper = '''

def layout_batch_size(model, options: dict[str, Any]) -> int:
    requested = options.get("layoutBatchSize")
    if requested is None:
        requested = os.environ.get("ROSETTA_PDF_LAYOUT_BATCH_SIZE")
    if requested is None:
        requested = getattr(model, "preferred_batch_size", 1)
    try:
        return max(1, min(5, int(requested)))
    except (TypeError, ValueError):
        return 1
'''
    if helper_anchor not in text:
        raise SystemExit(f"::error::could not find layout helper anchor in {target}")
    target.write_text(
        text.replace(helper_anchor, helper + helper_anchor, 1),
        encoding="utf-8",
    )
    print(f"[pdf2zh-pack] enabled bounded layout batching in {target}")
    return True


root = Path(pdf2zh.__file__).resolve().parent
patch_doclayout(root)
patch_engine(root)

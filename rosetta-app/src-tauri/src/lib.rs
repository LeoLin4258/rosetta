mod local_data_reset;
mod managed_pdf2zh;
mod managed_rwkv;
mod onboarding;
mod rosetta_jobs;
mod rwkv_api;
mod rwkv_providers;
#[allow(dead_code)]
mod rwkv_runtime;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(rwkv_api::RwkvTranslationRunRegistry::default())
        .manage(managed_rwkv::Registry::default())
        .manage(managed_rwkv::InstallStateRegistry::default())
        .manage(managed_pdf2zh::InstallStateRegistry::default())
        .manage(rosetta_jobs::PdfTranslationCancelState::default())
        .manage(rosetta_jobs::PdfPngCache::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Both `main` and `onboarding` are declared in tauri.conf.json
            // with `visible: false`. Pick one to show now based on whether
            // the user has completed onboarding AND the model is on disk.
            // Bundled `.app` launched from Finder lands here too — this is
            // the only entry point that decides "fresh user vs returning
            // user".
            let handle = app.handle();

            // Run one-time on-disk migrations *before* `onboarding::decide`,
            // because decide() inspects model presence. On beta.8 upgrades
            // from beta.7 this reclaims ~1.26 GB by removing the orphaned
            // WebRWKV 1.5B model directory; on fresh installs it's a no-op.
            // See `managed_rwkv::migrate` for the legacy artifact list.
            managed_rwkv::migrate::run_migrations(handle);

            let decision = onboarding::decide(handle);
            let target_label = if decision.needs_onboarding {
                "onboarding"
            } else {
                "main"
            };
            if let Some(window) = handle.get_webview_window(target_label) {
                window.show().ok();
                window.set_focus().ok();
            }

            // macOS native menu bar
            #[cfg(target_os = "macos")]
            {
                let menu = Menu::with_items(
                    app,
                    &[
                        // App menu (auto-named "Rosetta" by macOS)
                        &Submenu::with_items(
                            app,
                            "Rosetta",
                            true,
                            &[
                                &PredefinedMenuItem::about(app, None, None)?,
                                &PredefinedMenuItem::separator(app)?,
                                &MenuItem::with_id(
                                    app,
                                    "preferences",
                                    "Preferences...",
                                    true,
                                    Some("CmdOrCtrl+,"),
                                )?,
                                &PredefinedMenuItem::separator(app)?,
                                &PredefinedMenuItem::services(app, None)?,
                                &PredefinedMenuItem::separator(app)?,
                                &PredefinedMenuItem::hide(app, None)?,
                                &PredefinedMenuItem::hide_others(app, None)?,
                                &PredefinedMenuItem::show_all(app, None)?,
                                &PredefinedMenuItem::separator(app)?,
                                &PredefinedMenuItem::quit(app, None)?,
                            ],
                        )?,
                        // File menu
                        &Submenu::with_items(
                            app,
                            "File",
                            true,
                            &[
                                &MenuItem::with_id(
                                    app,
                                    "open-file",
                                    "Open...",
                                    true,
                                    Some("CmdOrCtrl+O"),
                                )?,
                                &PredefinedMenuItem::close_window(app, None)?,
                            ],
                        )?,
                        // Edit menu (gives system text-editing shortcuts for free)
                        &Submenu::with_items(
                            app,
                            "Edit",
                            true,
                            &[
                                &PredefinedMenuItem::undo(app, None)?,
                                &PredefinedMenuItem::redo(app, None)?,
                                &PredefinedMenuItem::separator(app)?,
                                &PredefinedMenuItem::cut(app, None)?,
                                &PredefinedMenuItem::copy(app, None)?,
                                &PredefinedMenuItem::paste(app, None)?,
                                &PredefinedMenuItem::select_all(app, None)?,
                            ],
                        )?,
                        // View menu
                        &Submenu::with_items(
                            app,
                            "View",
                            true,
                            &[&MenuItem::with_id(
                                app,
                                "toggle-sidebar",
                                "Toggle Sidebar",
                                true,
                                Some("CmdOrCtrl+\\"),
                            )?],
                        )?,
                        // Window menu
                        &Submenu::with_items(
                            app,
                            "Window",
                            true,
                            &[
                                &PredefinedMenuItem::minimize(app, None)?,
                                &PredefinedMenuItem::maximize(app, None)?,
                                &PredefinedMenuItem::fullscreen(app, None)?,
                            ],
                        )?,
                    ],
                )?;
                app.set_menu(menu)?;
            }

            Ok(())
        })
        .on_menu_event(|app, event| {
            let payload = event.id.as_ref().to_string();
            app.emit("rosetta-menu-event", payload).ok();
        })
        .on_window_event(|window, event| {
            // macOS: hide instead of destroy so the window can be restored
            // from the dock. Without this, close destroys the window handle,
            // Reopen can't find "main", and falls back to showing onboarding.
            #[cfg(target_os = "macos")]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            managed_rwkv::cancel_managed_rwkv_install,
            managed_rwkv::get_managed_rwkv_install_plan,
            managed_rwkv::get_managed_rwkv_install_progress,
            managed_rwkv::get_managed_rwkv_runtime_logs_summary,
            managed_rwkv::get_managed_rwkv_runtime_status,
            managed_rwkv::install_managed_rwkv_runtime,
            managed_rwkv::probe_managed_rwkv_runtime,
            managed_rwkv::start_managed_rwkv_runtime,
            managed_rwkv::stop_managed_rwkv_runtime,
            managed_pdf2zh::cancel_pdf2zh_install,
            managed_pdf2zh::get_pdf2zh_install_progress,
            managed_pdf2zh::get_pdf2zh_status,
            managed_pdf2zh::install_pdf2zh_pack,
            local_data_reset::clear_rosetta_local_data,
            onboarding::complete_onboarding_and_open_main,
            onboarding::get_onboarding_decision,
            onboarding::reopen_onboarding_window,
            rosetta_jobs::create_rosetta_translation_revision,
            rosetta_jobs::delete_rosetta_job,
            rosetta_jobs::delete_rosetta_job_file,
            rosetta_jobs::ensure_rosetta_translation_file,
            rosetta_jobs::export_rosetta_job_file,
            rosetta_jobs::export_rosetta_translated_pdf,
            rosetta_jobs::export_rosetta_translation_file,
            rosetta_jobs::create_welcome_document,
            rosetta_jobs::import_rosetta_document_from_path,
            rosetta_jobs::import_rosetta_project_from_directory,
            rosetta_jobs::list_rosetta_jobs,
            rosetta_jobs::load_rosetta_job,
            rosetta_jobs::load_rosetta_translation_file,
            rosetta_jobs::count_rosetta_pdf_pages,
            rosetta_jobs::cancel_rosetta_translated_pdf,
            rosetta_jobs::generate_rosetta_translated_pdf,
            rosetta_jobs::get_rosetta_pdf_assets,
            rosetta_jobs::get_rosetta_pdf_page_status,
            rosetta_jobs::pick_rosetta_export_path,
            rosetta_jobs::pick_rosetta_import_directory,
            rosetta_jobs::pick_rosetta_import_path,
            rosetta_jobs::probe_pdf_runtime,
            rosetta_jobs::read_rosetta_pdf_bytes,
            rosetta_jobs::render_rosetta_pdf_page_as_png,
            rosetta_jobs::render_rosetta_pdf_translated_page_as_png,
            rosetta_jobs::rename_rosetta_job,
            rosetta_jobs::save_rosetta_segments,
            rosetta_jobs::save_rosetta_translation_segments,
            rosetta_jobs::translate_rosetta_pdf_pages,
            rosetta_jobs::update_rosetta_job_file_languages,
            rwkv_api::cancel_rwkv_translation_run,
            rwkv_api::get_rwkv_translation_run_status,
            rwkv_api::probe_rwkv_mobile_batch_chat,
            rwkv_api::probe_rwkv_translation_api,
            rwkv_api::start_rwkv_mobile_batch_chat_run,
            rwkv_api::start_rwkv_translation_run,
            rwkv_api::translate_rwkv_mobile_batch_chat_texts,
            rwkv_api::translate_rwkv_texts_with_api
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // macOS: clicking the dock icon while all windows are closed
            // fires Reopen. Without handling it, the app sits in the dock
            // with the running-dot but no way to surface the window short
            // of right-click → Quit. Re-show whichever window we previously
            // chose at startup (main or onboarding).
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows,
                ..
            } = event
            {
                if has_visible_windows {
                    return;
                }
                let target = app_handle
                    .get_webview_window("main")
                    .or_else(|| app_handle.get_webview_window("onboarding"));
                if let Some(window) = target {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app_handle, event);
        });
}

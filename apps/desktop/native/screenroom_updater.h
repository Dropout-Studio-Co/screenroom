#ifndef SCREENROOM_UPDATER_H
#define SCREENROOM_UPDATER_H

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void *ScreenroomUpdaterHandle;

typedef void (*ScreenroomUpdaterEventCallback)(const char *event_json);

typedef enum ScreenroomUpdateChoice {
    ScreenroomUpdateChoiceInstall = 0,
    ScreenroomUpdateChoiceDismiss = 1,
    ScreenroomUpdateChoiceSkip = 2,
} ScreenroomUpdateChoice;

ScreenroomUpdaterHandle screenroom_updater_create(
    const char *configuration_json,
    ScreenroomUpdaterEventCallback callback);

void screenroom_updater_destroy(ScreenroomUpdaterHandle handle);

bool screenroom_updater_check_now(ScreenroomUpdaterHandle handle);

bool screenroom_updater_check_in_background(ScreenroomUpdaterHandle handle);

bool screenroom_updater_choose(ScreenroomUpdaterHandle handle, int choice);

bool screenroom_updater_install_now(ScreenroomUpdaterHandle handle);

bool screenroom_updater_defer_to_quit(ScreenroomUpdaterHandle handle);

bool screenroom_updater_cancel_download(ScreenroomUpdaterHandle handle);

bool screenroom_updater_install_pending_on_quit(ScreenroomUpdaterHandle handle);

bool screenroom_updater_can_check(ScreenroomUpdaterHandle handle);

bool screenroom_updater_session_in_progress(ScreenroomUpdaterHandle handle);

bool screenroom_updater_automatic_checks(ScreenroomUpdaterHandle handle);

bool screenroom_updater_automatic_downloads(ScreenroomUpdaterHandle handle);

bool screenroom_updater_allows_automatic_updates(ScreenroomUpdaterHandle handle);

bool screenroom_updater_set_automatic_checks(ScreenroomUpdaterHandle handle, bool enabled);

bool screenroom_updater_set_automatic_downloads(ScreenroomUpdaterHandle handle, bool enabled);

void screenroom_updater_reset_cycle(ScreenroomUpdaterHandle handle);

void screenroom_updater_set_relaunch_after_install(ScreenroomUpdaterHandle handle, bool enabled);

#ifdef __cplusplus
}
#endif

#endif

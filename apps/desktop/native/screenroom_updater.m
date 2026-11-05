#import "screenroom_updater.h"

#import <Foundation/Foundation.h>
#import <Sparkle/Sparkle.h>

static NSString *const ScreenroomUpdaterEventNameKey = @"event";

static NSString *const ScreenroomUpdaterVersionKey = @"version";

static NSString *const ScreenroomUpdaterStageNotDownloaded = @"not_downloaded";

static NSString *const ScreenroomUpdaterStageDownloaded = @"downloaded";

static NSString *const ScreenroomUpdaterStageInstalling = @"installing";

static NSString *const ScreenroomUpdaterUnknownErrorText = @"The updater reported a failure without a description.";

static NSString *ScreenroomUpdaterStageName(SPUUserUpdateStage stage)
{
    switch (stage) {
        case SPUUserUpdateStageNotDownloaded:
            return ScreenroomUpdaterStageNotDownloaded;
        case SPUUserUpdateStageDownloaded:
            return ScreenroomUpdaterStageDownloaded;
        case SPUUserUpdateStageInstalling:
            return ScreenroomUpdaterStageInstalling;
    }
    return ScreenroomUpdaterStageNotDownloaded;
}

static NSString *ScreenroomUpdaterSigningStatusName(SPUAppcastSigningValidationStatus status)
{
    switch (status) {
        case SPUAppcastSigningValidationStatusSkipped:
            return @"skipped";
        case SPUAppcastSigningValidationStatusSucceeded:
            return @"succeeded";
        case SPUAppcastSigningValidationStatusFailed:
            return @"failed";
    }
    return @"unknown";
}

@interface ScreenroomUpdateController : NSObject <SPUUserDriver, SPUUpdaterDelegate>

@property (nonatomic) ScreenroomUpdaterEventCallback callback;

@property (nonatomic, strong, nullable) SPUUpdater *updater;

@property (nonatomic, copy, nullable) void (^pendingUpdateReply)(SPUUserUpdateChoice);

@property (nonatomic, copy, nullable) void (^pendingReadyReply)(SPUUserUpdateChoice);

@property (nonatomic, copy, nullable) void (^pendingCancellation)(void);

@property (nonatomic, copy, nullable) void (^pendingAcknowledgement)(void);

@property (nonatomic, copy, nullable) void (^pendingQuitInstall)(void);

@property (nonatomic, copy, nullable) NSString *announcedVersion;

@property (nonatomic, copy, nullable) NSString *announcedDisplayVersion;

@property (nonatomic, copy, nullable) NSString *feedURLOverride;

@property (nonatomic) BOOL relaunchAfterInstall;

@property (nonatomic) BOOL announcingUserCheck;

@property (nonatomic) BOOL announcedUpdate;

@property (nonatomic) uint64_t downloadedBytes;

@property (nonatomic) uint64_t expectedBytes;

@property (nonatomic) CFAbsoluteTime lastProgressAnnouncement;

@end

@implementation ScreenroomUpdateController

- (instancetype)initWithConfigurationJSON:(nullable NSString *)configuration
{
    self = [super init];
    if (self == nil) {
        return nil;
    }
    _relaunchAfterInstall = YES;
    _lastProgressAnnouncement = 0;
    [self readConfiguration:configuration];
    return self;
}

- (void)readConfiguration:(nullable NSString *)configuration
{
    if (configuration.length == 0) {
        return;
    }
    NSData *data = [configuration dataUsingEncoding:NSUTF8StringEncoding];
    if (data == nil) {
        return;
    }
    id parsed = [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
    if (![parsed isKindOfClass:[NSDictionary class]]) {
        return;
    }
    NSDictionary *document = (NSDictionary *)parsed;
    id feed = document[@"feed_url"];
    if ([feed isKindOfClass:[NSString class]] && ((NSString *)feed).length > 0) {
        self.feedURLOverride = feed;
    }
    id relaunch = document[@"relaunch_after_install"];
    if ([relaunch isKindOfClass:[NSNumber class]]) {
        self.relaunchAfterInstall = [relaunch boolValue];
    }
}

- (BOOL)startsUpdater
{
    NSError *error = nil;
    if (![self.updater startUpdater:&error]) {
        [self announce:@"failed" details:@{
            @"message": error.localizedDescription ?: ScreenroomUpdaterUnknownErrorText,
            @"kind": @"startup",
        }];
        return NO;
    }
    [self announceSettings];
    return YES;
}

- (void)announceSettings
{
    [self announce:@"settings" details:@{
        @"automatic_checks": @(self.updater.automaticallyChecksForUpdates),
        @"automatic_downloads": @(self.updater.automaticallyDownloadsUpdates),
        @"allows_automatic_updates": @(self.updater.allowsAutomaticUpdates),
        @"can_check": @(self.updater.canCheckForUpdates),
        @"session_in_progress": @(self.updater.sessionInProgress),
    }];
}

- (void)announce:(NSString *)name details:(nullable NSDictionary<NSString *, id> *)details
{
    ScreenroomUpdaterEventCallback callback = self.callback;
    if (callback == NULL) {
        return;
    }
    NSMutableDictionary *document = [NSMutableDictionary dictionaryWithCapacity:(details.count + 1)];
    document[ScreenroomUpdaterEventNameKey] = name;
    for (NSString *key in details) {
        id value = details[key];
        if (value != nil) {
            document[key] = value;
        }
    }
    NSData *json = [NSJSONSerialization dataWithJSONObject:document options:0 error:NULL];
    if (json == nil) {
        return;
    }
    NSString *text = [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
    if (text == nil) {
        return;
    }
    callback(text.UTF8String);
}

- (void)announceUserCheckIfNeeded
{
    if (self.updater.canCheckForUpdates || self.updater.sessionInProgress) {
        return;
    }
    [self announce:@"idle" details:@{ @"can_check": @(self.updater.canCheckForUpdates) }];
}

- (void)resolveUpdateReply:(SPUUserUpdateChoice)choice
{
    void (^reply)(SPUUserUpdateChoice) = self.pendingUpdateReply;
    self.pendingUpdateReply = nil;
    if (reply != nil) {
        reply(choice);
    }
}

- (void)resolveReadyReply:(SPUUserUpdateChoice)choice
{
    void (^reply)(SPUUserUpdateChoice) = self.pendingReadyReply;
    self.pendingReadyReply = nil;
    if (reply != nil) {
        reply(choice);
    }
}

- (void)resolveAcknowledgement
{
    void (^acknowledgement)(void) = self.pendingAcknowledgement;
    self.pendingAcknowledgement = nil;
    if (acknowledgement != nil) {
        acknowledgement();
    }
}

- (void)announceProgressForcefully:(BOOL)forcefully
{
    CFAbsoluteTime now = CFAbsoluteTimeGetCurrent();
    if (!forcefully && (now - self.lastProgressAnnouncement) < 0.25) {
        return;
    }
    self.lastProgressAnnouncement = now;
    [self announce:@"downloading" details:@{
        @"received": @(self.downloadedBytes),
        @"expected": @(self.expectedBytes),
        @"version": self.announcedVersion ?: @"",
    }];
}

- (void)showUpdatePermissionRequest:(SPUUpdatePermissionRequest *)request reply:(void (^)(SUUpdatePermissionResponse *))reply
{
    reply([[SUUpdatePermissionResponse alloc] initWithAutomaticUpdateChecks:YES automaticUpdateDownloading:nil sendSystemProfile:NO]);
    [self announceSettings];
}

- (void)showUserInitiatedUpdateCheckWithCancellation:(void (^)(void))cancellation
{
    self.pendingCancellation = cancellation;
    self.announcingUserCheck = YES;
    self.announcedUpdate = NO;
    [self announce:@"checking" details:@{ @"user_initiated": @YES }];
}

- (void)showUpdateFoundWithAppcastItem:(SUAppcastItem *)appcastItem state:(SPUUserUpdateState *)state reply:(void (^)(SPUUserUpdateChoice))reply
{
    self.pendingUpdateReply = reply;
    self.announcingUserCheck = state.userInitiated;
    self.announcedUpdate = YES;
    self.announcedVersion = appcastItem.versionString;
    self.announcedDisplayVersion = appcastItem.displayVersionString ?: appcastItem.versionString;
    self.downloadedBytes = 0;
    self.expectedBytes = appcastItem.contentLength;

    [self announce:@"found" details:@{
        @"version": appcastItem.versionString ?: @"",
        @"display_version": self.announcedDisplayVersion ?: @"",
        @"title": appcastItem.title ?: @"",
        @"notes": appcastItem.itemDescription ?: @"",
        @"notes_format": appcastItem.itemDescriptionFormat ?: @"",
        @"stage": ScreenroomUpdaterStageName(state.stage),
        @"user_initiated": @(state.userInitiated),
        @"critical": @(appcastItem.isCriticalUpdate),
        @"informational": @(appcastItem.isInformationOnlyUpdate),
        @"major_upgrade": @(appcastItem.isMajorUpgrade),
        @"content_length": @(appcastItem.contentLength),
        @"feed_signing": ScreenroomUpdaterSigningStatusName(appcastItem.signingValidationStatus),
        @"info_url": appcastItem.infoURL.absoluteString ?: @"",
    }];

    if (appcastItem.isInformationOnlyUpdate) {
        [self resolveUpdateReply:SPUUserUpdateChoiceDismiss];
        return;
    }
    if (!state.userInitiated && self.updater.automaticallyDownloadsUpdates) {
        [self resolveUpdateReply:SPUUserUpdateChoiceInstall];
        return;
    }
    if (!state.userInitiated) {
        [self resolveUpdateReply:SPUUserUpdateChoiceDismiss];
    }
}

- (void)showUpdateReleaseNotesWithDownloadData:(SPUDownloadData *)downloadData
{
}

- (void)showUpdateReleaseNotesFailedToDownloadWithError:(NSError *)error
{
}

- (void)showUpdateNotFoundWithError:(NSError *)error acknowledgement:(void (^)(void))acknowledgement
{
    self.pendingAcknowledgement = acknowledgement;
    [self announce:@"up_to_date" details:@{
        @"user_initiated": @(self.announcingUserCheck),
        @"message": error.localizedDescription ?: @"",
    }];
    [self resolveAcknowledgement];
    [self announceSettings];
}

- (void)showUpdaterError:(NSError *)error acknowledgement:(void (^)(void))acknowledgement
{
    self.pendingAcknowledgement = acknowledgement;
    NSString *recovery = error.userInfo[NSLocalizedRecoverySuggestionErrorKey];
    [self announce:@"failed" details:@{
        @"message": error.localizedDescription ?: ScreenroomUpdaterUnknownErrorText,
        @"recovery": recovery ?: @"",
        @"domain": error.domain ?: @"",
        @"code": @(error.code),
        @"version": self.announcedVersion ?: @"",
    }];
    self.pendingUpdateReply = nil;
    self.pendingReadyReply = nil;
    [self resolveAcknowledgement];
    [self announceSettings];
}

- (void)showDownloadInitiatedWithCancellation:(void (^)(void))cancellation
{
    self.pendingCancellation = cancellation;
    self.downloadedBytes = 0;
    [self announceProgressForcefully:YES];
}

- (void)showDownloadDidReceiveExpectedContentLength:(uint64_t)expectedContentLength
{
    self.expectedBytes = expectedContentLength;
    [self announceProgressForcefully:YES];
}

- (void)showDownloadDidReceiveDataOfLength:(uint64_t)length
{
    self.downloadedBytes += length;
    [self announceProgressForcefully:NO];
}

- (void)showDownloadDidStartExtractingUpdate
{
    self.pendingCancellation = nil;
    [self announce:@"extracting" details:@{
        @"progress": @0.0,
        @"version": self.announcedVersion ?: @"",
    }];
}

- (void)showExtractionReceivedProgress:(double)progress
{
    [self announce:@"extracting" details:@{
        @"progress": @(progress),
        @"version": self.announcedVersion ?: @"",
    }];
}

- (void)showReadyToInstallAndRelaunch:(void (^)(SPUUserUpdateChoice))reply
{
    self.pendingReadyReply = reply;
    [self announce:@"ready" details:@{
        @"version": self.announcedVersion ?: @"",
        @"display_version": self.announcedDisplayVersion ?: @"",
        @"user_initiated": @(self.announcingUserCheck),
    }];
    if (!self.announcingUserCheck) {
        [self deferReadyUpdateToQuit];
    }
}

- (void)deferReadyUpdateToQuit
{
    self.relaunchAfterInstall = NO;
    [self announce:@"deferred" details:@{
        @"version": self.announcedVersion ?: @"",
        @"display_version": self.announcedDisplayVersion ?: @"",
    }];
    [self resolveReadyReply:SPUUserUpdateChoiceDismiss];
    [self announceSettings];
}

- (void)showInstallingUpdateWithApplicationTerminated:(BOOL)applicationTerminated retryTerminatingApplication:(void (^)(void))retryTerminatingApplication
{
    [self announce:@"installing" details:@{
        @"application_terminated": @(applicationTerminated),
        @"version": self.announcedVersion ?: @"",
    }];
}

- (void)showUpdateInstalledAndRelaunched:(BOOL)relaunched acknowledgement:(void (^)(void))acknowledgement
{
    self.pendingAcknowledgement = acknowledgement;
    [self announce:@"installed" details:@{
        @"relaunched": @(relaunched),
        @"version": self.announcedVersion ?: @"",
    }];
    [self resolveAcknowledgement];
}

- (void)dismissUpdateInstallation
{
    self.pendingUpdateReply = nil;
    self.pendingReadyReply = nil;
    self.pendingCancellation = nil;
    self.announcedUpdate = NO;
    [self announce:@"dismissed" details:@{ @"version": self.announcedVersion ?: @"" }];
}

- (BOOL)updater:(SPUUpdater *)updater willInstallUpdateOnQuit:(SUAppcastItem *)item immediateInstallationBlock:(void (^)(void))immediateInstallHandler
{
    self.pendingQuitInstall = immediateInstallHandler;
    [self announce:@"install_on_quit" details:@{
        @"version": item.versionString ?: @"",
        @"display_version": item.displayVersionString ?: item.versionString ?: @"",
    }];
    return YES;
}

- (BOOL)updaterShouldRelaunchApplication:(SPUUpdater *)updater
{
    return self.relaunchAfterInstall;
}

- (void)updater:(SPUUpdater *)updater willScheduleUpdateCheckAfterDelay:(NSTimeInterval)delay
{
    [self announce:@"scheduled" details:@{ @"delay": @(delay) }];
}

- (void)updaterWillNotScheduleUpdateCheck:(SPUUpdater *)updater
{
    [self announce:@"unscheduled" details:nil];
}

- (nullable NSString *)feedURLStringForUpdater:(SPUUpdater *)updater
{
    return self.feedURLOverride;
}

- (void)updater:(SPUUpdater *)updater didFinishUpdateCycleForUpdateCheck:(SPUUpdateCheck)updateCheck error:(nullable NSError *)error
{
    [self announceSettings];
}

@end

static BOOL ScreenroomUpdaterIsReady(ScreenroomUpdaterHandle handle)
{
    return handle != NULL && [NSThread isMainThread];
}

ScreenroomUpdaterHandle screenroom_updater_create(const char *configuration_json, ScreenroomUpdaterEventCallback callback)
{
    if (![NSThread isMainThread]) {
        return NULL;
    }
    if (NSClassFromString(@"SPUUpdater") == nil || NSClassFromString(@"SPUStandardUserDriver") == nil) {
        return NULL;
    }
    NSString *configuration = configuration_json == NULL
        ? nil
        : [NSString stringWithUTF8String:configuration_json];
    ScreenroomUpdateController *controller = [[ScreenroomUpdateController alloc] initWithConfigurationJSON:configuration];
    if (controller == nil) {
        return NULL;
    }
    controller.callback = callback;
    NSBundle *bundle = [NSBundle mainBundle];
    SPUUpdater *updater = [[SPUUpdater alloc] initWithHostBundle:bundle applicationBundle:bundle userDriver:controller delegate:controller];
    controller.updater = updater;
    if (![controller startsUpdater]) {
        return NULL;
    }
    return (ScreenroomUpdaterHandle)CFBridgingRetain(controller);
}

void screenroom_updater_destroy(ScreenroomUpdaterHandle handle)
{
    if (handle == NULL) {
        return;
    }
    ScreenroomUpdateController *controller = (__bridge_transfer ScreenroomUpdateController *)handle;
    controller.callback = NULL;
}

bool screenroom_updater_check_now(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    if (controller.updater == nil || !controller.updater.canCheckForUpdates) {
        return false;
    }
    [controller.updater checkForUpdates];
    return true;
}

bool screenroom_updater_choose(ScreenroomUpdaterHandle handle, int choice)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    SPUUserUpdateChoice resolved = SPUUserUpdateChoiceDismiss;
    switch (choice) {
        case ScreenroomUpdateChoiceInstall:
            resolved = SPUUserUpdateChoiceInstall;
            break;
        case ScreenroomUpdateChoiceDismiss:
            resolved = SPUUserUpdateChoiceDismiss;
            break;
        case ScreenroomUpdateChoiceSkip:
            resolved = SPUUserUpdateChoiceSkip;
            break;
    }
    BOOL hadReadyReply = controller.pendingReadyReply != nil;
    BOOL hadUpdateReply = controller.pendingUpdateReply != nil;
    [controller resolveReadyReply:resolved];
    [controller resolveUpdateReply:resolved];
    return hadReadyReply || hadUpdateReply;
}

bool screenroom_updater_install_now(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    controller.relaunchAfterInstall = NO;
    return screenroom_updater_choose(handle, ScreenroomUpdateChoiceInstall);
}

bool screenroom_updater_defer_to_quit(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    controller.relaunchAfterInstall = NO;
    return screenroom_updater_choose(handle, ScreenroomUpdateChoiceDismiss);
}

bool screenroom_updater_install_pending_on_quit(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    void (^handler)(void) = controller.pendingQuitInstall;
    controller.pendingQuitInstall = nil;
    if (handler == nil) {
        return false;
    }
    [controller announce:@"quit_install_started" details:nil];
    handler();
    return true;
}

bool screenroom_updater_cancel_download(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    void (^cancellation)(void) = controller.pendingCancellation;
    controller.pendingCancellation = nil;
    if (cancellation == nil) {
        return false;
    }
    cancellation();
    return true;
}

bool screenroom_updater_can_check(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    return controller.updater.canCheckForUpdates;
}

bool screenroom_updater_session_in_progress(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    return controller.updater.sessionInProgress;
}

bool screenroom_updater_automatic_checks(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    return controller.updater.automaticallyChecksForUpdates;
}

bool screenroom_updater_automatic_downloads(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    return controller.updater.automaticallyDownloadsUpdates;
}

bool screenroom_updater_allows_automatic_updates(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    return controller.updater.allowsAutomaticUpdates;
}

bool screenroom_updater_set_automatic_checks(ScreenroomUpdaterHandle handle, bool enabled)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    controller.updater.automaticallyChecksForUpdates = enabled;
    [controller announceSettings];
    return true;
}

bool screenroom_updater_set_automatic_downloads(ScreenroomUpdaterHandle handle, bool enabled)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return false;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    controller.updater.automaticallyDownloadsUpdates = enabled;
    [controller announceSettings];
    return true;
}

void screenroom_updater_reset_cycle(ScreenroomUpdaterHandle handle)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    [controller.updater resetUpdateCycleAfterShortDelay];
}

void screenroom_updater_set_relaunch_after_install(ScreenroomUpdaterHandle handle, bool enabled)
{
    if (!ScreenroomUpdaterIsReady(handle)) {
        return;
    }
    ScreenroomUpdateController *controller = (__bridge ScreenroomUpdateController *)handle;
    controller.relaunchAfterInstall = enabled;
}

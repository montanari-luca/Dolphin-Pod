import BackgroundTasks
import SwiftUI

@main
struct DolphinPodApp: App {
    @State private var model = AppModel()
    @Environment(\.scenePhase) private var phase

    init() {
        // Background refresh asks the core to sync so the app opens up to date.
        BGTaskScheduler.shared.register(forTaskWithIdentifier: BackgroundSync.identifier, using: nil) { task in
            BackgroundSync.handle(task as! BGAppRefreshTask)
        }
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .tint(Brand.accent)
                .task {
                    BackgroundSync.model = model
                    await model.boot()
                }
                .onChange(of: phase) { _, p in
                    switch p {
                    case .active: model.foreground()
                    case .background: model.background(); BackgroundSync.schedule()
                    default: break
                    }
                }
        }
    }
}

enum BackgroundSync {
    static let identifier = "dev.mintflow.dolphinpod.refresh"
    @MainActor static var model: AppModel?

    static func schedule() {
        let request = BGAppRefreshTaskRequest(identifier: identifier)
        request.earliestBeginDate = Date(timeIntervalSinceNow: 15 * 60)
        try? BGTaskScheduler.shared.submit(request)
    }

    static func handle(_ task: BGAppRefreshTask) {
        schedule()
        let work = Task { @MainActor in
            try? await model?.client?.syncNow()
            task.setTaskCompleted(success: true)
        }
        task.expirationHandler = { work.cancel() }
    }
}

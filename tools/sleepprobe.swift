// Measures how long a 16 ms sleep really takes under three scheduling classes.
// Usage: sleepprobe <seconds>. One line per class per second: name, wakes, mean ms, max ms.
import CoreVideo
import Darwin
import Foundation

let seconds = Int(CommandLine.arguments.dropFirst().first ?? "60") ?? 60
let target: useconds_t = 16_667

func timeConstraint() -> Bool {
    var info = mach_timebase_info_data_t()
    mach_timebase_info(&info)
    let toAbs = { (ns: Double) -> UInt32 in UInt32(ns * Double(info.denom) / Double(info.numer)) }
    var policy = thread_time_constraint_policy_data_t(
        period: toAbs(16_666_667), computation: toAbs(2_000_000),
        constraint: toAbs(16_666_667), preemptible: 1)
    let count = mach_msg_type_number_t(
        MemoryLayout<thread_time_constraint_policy_data_t>.size / MemoryLayout<integer_t>.size)
    let result = withUnsafeMutablePointer(to: &policy) {
        $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
            thread_policy_set(
                pthread_mach_thread_np(pthread_self()),
                thread_policy_flavor_t(THREAD_TIME_CONSTRAINT_POLICY), $0, count)
        }
    }
    return result == KERN_SUCCESS
}

func run(_ name: String, setup: @escaping () -> Void) {
    Thread.detachNewThread {
        setup()
        let start = Date()
        var wakes = 0
        var total = 0.0
        var worst = 0.0
        var mark = Date()
        while Date().timeIntervalSince(start) < Double(seconds) {
            let before = Date()
            usleep(target)
            let ms = Date().timeIntervalSince(before) * 1000
            wakes += 1
            total += ms
            worst = max(worst, ms)
            if Date().timeIntervalSince(mark) >= 1 {
                let t = Int(Date().timeIntervalSince(start) * 1000)
                print("\(t)\t\(name)\twakes=\(wakes)\tmean=\(String(format: "%.1f", total / Double(wakes)))\tmax=\(String(format: "%.1f", worst))")
                fflush(stdout)
                wakes = 0
                total = 0
                worst = 0
                mark = Date()
            }
        }
    }
}

// Vsync-driven wakes: the display link callback signals a waiting thread.
let vsync = DispatchSemaphore(value: 0)
var link: CVDisplayLink?
CVDisplayLinkCreateWithActiveCGDisplays(&link)
if let link {
    CVDisplayLinkSetOutputHandler(link) { _, _, _, _, _ in
        vsync.signal()
        return kCVReturnSuccess
    }
    CVDisplayLinkStart(link)
}
Thread.detachNewThread {
    let start = Date()
    var wakes = 0
    var total = 0.0
    var worst = 0.0
    var mark = Date()
    var last = Date()
    while Date().timeIntervalSince(start) < Double(seconds) {
        vsync.wait()
        let now = Date()
        let ms = now.timeIntervalSince(last) * 1000
        last = now
        wakes += 1
        total += ms
        worst = max(worst, ms)
        if now.timeIntervalSince(mark) >= 1 {
            let t = Int(now.timeIntervalSince(start) * 1000)
            print("\(t)\tvsync\twakes=\(wakes)\tmean=\(String(format: "%.1f", total / Double(wakes)))\tmax=\(String(format: "%.1f", worst))")
            fflush(stdout)
            wakes = 0
            total = 0
            worst = 0
            mark = now
        }
    }
}

run("default") {}
run("interactive") { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) }
run("realtime") { print("realtime policy ok=\(timeConstraint())") }
Thread.sleep(forTimeInterval: Double(seconds) + 1)

use std::time::{Duration, Instant};
use sysinfo;
use terminator_workflow_recorder::{WorkflowEvent, WorkflowRecorder, WorkflowRecorderConfig};
use tokio_stream::StreamExt;

/// Performance test that measures CPU load and typing performance during Terminator keyboard automation
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_typing_performance_and_cpu_load() {
        use std::sync::{
            atomic::{AtomicU32, AtomicU64, Ordering},
            Arc,
        };
        use std::thread;

        println!("🚀 Testing TYPING PERFORMANCE and CPU load during keyboard automation...");
        println!("💡 This measures CPU usage and typing lag while recording keyboard events");

        // Start recorder with keyboard-focused settings
        let config = WorkflowRecorderConfig {
            record_mouse: false, // Disable to focus on keyboard performance
            record_keyboard: true,
            record_text_input_completion: true,
            capture_ui_elements: true, // This might be causing lag
            record_clipboard: false,
            ..Default::default()
        };

        let mut recorder = WorkflowRecorder::new("Typing Performance Test".to_string(), config);
        let mut event_stream = recorder.event_stream();

        // CPU and Memory monitoring
        let cpu_samples = Arc::new(AtomicU32::new(0));
        let memory_samples = Arc::new(AtomicU64::new(0));
        let cpu_counter_for_thread = Arc::clone(&cpu_samples);
        let memory_counter = Arc::clone(&memory_samples);
        let stop_monitoring = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop_monitoring);

        let monitoring_thread = thread::spawn(move || {
            let mut system = sysinfo::System::new_all();
            let mut max_memory = 0u64;
            let mut max_cpu = 0.0f32;
            let mut sample_count = 0u32;

            while !stop_flag.load(Ordering::Relaxed) {
                system.refresh_process(sysinfo::get_current_pid().unwrap());
                if let Some(process) = system.process(sysinfo::get_current_pid().unwrap()) {
                    let memory_kb = process.memory() / 1024;
                    let cpu_percent = process.cpu_usage();

                    max_memory = max_memory.max(memory_kb);
                    max_cpu = max_cpu.max(cpu_percent);
                    sample_count += 1;

                    if sample_count % 50 == 0 {
                        println!("📊 CPU: {:.1}%, Memory: {} KB", cpu_percent, memory_kb);
                    }
                }
                thread::sleep(Duration::from_millis(100)); // Sample every 100ms
            }
            memory_counter.store(max_memory, Ordering::Relaxed);
            cpu_counter_for_thread.store((max_cpu * 100.0) as u32, Ordering::Relaxed);
            // Store as percentage * 100
        });

        // Start recording
        let start_time = Instant::now();
        recorder.start().await.expect("Failed to start recorder");
        println!("✅ Recording started in {:?}", start_time.elapsed());

        // TYPING PERFORMANCE TEST WITH TERMINATOR
        let typing_handle = tokio::spawn(async move {
            println!("⌨️ Starting TYPING PERFORMANCE test with Terminator...");

            tokio::time::sleep(Duration::from_millis(500)).await;

            match terminator::Desktop::new(false, false) {
                Ok(desktop) => {
                    println!("📝 Opening Notepad for typing test...");
                    match desktop.open_application("notepad") {
                        Ok(_) => {
                            tokio::time::sleep(Duration::from_millis(2000)).await;

                            match desktop
                                .locator("name:Text Editor")
                                .first(Some(Duration::from_millis(3000)))
                                .await
                            {
                                Ok(text_element) => {
                                    println!(
                                        "✅ Found text editor, starting typing performance test..."
                                    );

                                    // Test 1: Slow typing (realistic user speed)
                                    println!("\n🐌 TEST 1: Slow typing (150ms delays)");
                                    let slow_text = "Slow typing test: This simulates normal user typing speed to measure CPU impact.";
                                    let slow_start = Instant::now();

                                    for (i, ch) in slow_text.chars().enumerate() {
                                        let char_start = Instant::now();
                                        if let Err(_) =
                                            text_element.type_text(&ch.to_string(), false)
                                        {
                                            println!("⚠️ Type error at char {}", i);
                                        }
                                        let char_duration = char_start.elapsed();

                                        if char_duration > Duration::from_millis(50) {
                                            println!("🐌 SLOW char {}: {:?}", ch, char_duration);
                                        }

                                        tokio::time::sleep(Duration::from_millis(150)).await;
                                    }
                                    let slow_duration = slow_start.elapsed();
                                    println!(
                                        "🐌 Slow typing completed in {:?} ({:.1} chars/sec)",
                                        slow_duration,
                                        slow_text.len() as f64 / slow_duration.as_secs_f64()
                                    );

                                    tokio::time::sleep(Duration::from_millis(1000)).await;

                                    // Test 2: Medium typing
                                    println!("\n⚡ TEST 2: Medium typing (80ms delays)");
                                    let medium_text = "\n\nMedium typing test: Faster typing to stress test recording performance.";
                                    let medium_start = Instant::now();

                                    for (i, ch) in medium_text.chars().enumerate() {
                                        let char_start = Instant::now();
                                        if let Err(_) =
                                            text_element.type_text(&ch.to_string(), false)
                                        {
                                            println!("⚠️ Type error at char {}", i);
                                        }
                                        let char_duration = char_start.elapsed();

                                        if char_duration > Duration::from_millis(30) {
                                            println!("⚡ SLOW char {}: {:?}", ch, char_duration);
                                        }

                                        tokio::time::sleep(Duration::from_millis(80)).await;
                                    }
                                    let medium_duration = medium_start.elapsed();
                                    println!(
                                        "⚡ Medium typing completed in {:?} ({:.1} chars/sec)",
                                        medium_duration,
                                        medium_text.len() as f64 / medium_duration.as_secs_f64()
                                    );

                                    tokio::time::sleep(Duration::from_millis(1000)).await;

                                    // Test 3: Fast typing (stress test)
                                    println!("\n🔥 TEST 3: Fast typing (50ms delays)");
                                    let fast_text = "\n\nFast typing stress test: Maximum speed to identify bottlenecks!!!";
                                    let fast_start = Instant::now();
                                    let mut slow_chars = 0;

                                    for (i, ch) in fast_text.chars().enumerate() {
                                        let char_start = Instant::now();
                                        if let Err(_) =
                                            text_element.type_text(&ch.to_string(), false)
                                        {
                                            println!("⚠️ Type error at char {}", i);
                                        }
                                        let char_duration = char_start.elapsed();

                                        if char_duration > Duration::from_millis(20) {
                                            slow_chars += 1;
                                            if char_duration > Duration::from_millis(100) {
                                                println!(
                                                    "🔥 VERY SLOW char {}: {:?}",
                                                    ch, char_duration
                                                );
                                            }
                                        }

                                        tokio::time::sleep(Duration::from_millis(50)).await;
                                    }
                                    let fast_duration = fast_start.elapsed();
                                    println!(
                                        "🔥 Fast typing completed in {:?} ({:.1} chars/sec)",
                                        fast_duration,
                                        fast_text.len() as f64 / fast_duration.as_secs_f64()
                                    );
                                    println!(
                                        "🔥 Slow characters (>20ms): {}/{}",
                                        slow_chars,
                                        fast_text.len()
                                    );

                                    // Test 4: Burst typing (simulate user bursts)
                                    println!(
                                        "\n💥 TEST 4: Burst typing (10 chars rapidly, then pause)"
                                    );
                                    let burst_text = "\n\nBurst test: abcdefghij pause abcdefghij pause abcdefghij";
                                    let burst_start = Instant::now();

                                    for (i, ch) in burst_text.chars().enumerate() {
                                        let char_start = Instant::now();
                                        if let Err(_) =
                                            text_element.type_text(&ch.to_string(), false)
                                        {
                                            println!("⚠️ Type error at char {}", i);
                                        }
                                        let char_duration = char_start.elapsed();

                                        if char_duration > Duration::from_millis(15) {
                                            println!(
                                                "💥 SLOW burst char {}: {:?}",
                                                ch, char_duration
                                            );
                                        }

                                        // Burst pattern: 10 fast chars, then pause
                                        if (i + 1) % 10 == 0 {
                                            tokio::time::sleep(Duration::from_millis(500)).await;
                                        // Pause
                                        } else {
                                            tokio::time::sleep(Duration::from_millis(30)).await;
                                            // Fast
                                        }
                                    }
                                    let burst_duration = burst_start.elapsed();
                                    println!(
                                        "💥 Burst typing completed in {:?} ({:.1} chars/sec)",
                                        burst_duration,
                                        burst_text.len() as f64 / burst_duration.as_secs_f64()
                                    );

                                    println!("\n✅ All typing performance tests completed!");
                                }
                                Err(e) => {
                                    println!("❌ Could not find text editor: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            println!("❌ Failed to open Notepad: {}", e);
                        }
                    }
                }
                Err(e) => {
                    println!("❌ Failed to initialize Terminator: {}", e);
                }
            }
        });

        // Event collection with detailed keyboard tracking
        let mut event_count = 0;
        let mut keyboard_events = 0;
        let mut text_completion_events = 0;
        let mut total_keystroke_processing_time = Duration::new(0, 0);

        println!("📊 Monitoring keyboard events and processing times...");
        let event_collection = tokio::time::timeout(Duration::from_secs(45), async {
            while let Some(event) = event_stream.next().await {
                let event_start = Instant::now();
                event_count += 1;

                match event {
                    WorkflowEvent::Keyboard(_) => {
                        keyboard_events += 1;
                        let processing_time = event_start.elapsed();
                        total_keystroke_processing_time += processing_time;

                        if processing_time > Duration::from_millis(10) {
                            println!("🐌 SLOW keyboard event processing: {:?}", processing_time);
                        }

                        if keyboard_events % 50 == 0 {
                            println!("⌨️ Processed {} keyboard events", keyboard_events);
                        }
                    }
                    WorkflowEvent::TextInputCompleted(_) => {
                        text_completion_events += 1;
                        println!(
                            "🔥 TEXT COMPLETION EVENT! (Total: {})",
                            text_completion_events
                        );
                    }
                    _ => {}
                }

                if event_count >= 5000 {
                    println!("🛑 Stopping at 5000 events");
                    break;
                }
            }
            (
                event_count,
                keyboard_events,
                text_completion_events,
                total_keystroke_processing_time,
            )
        });

        let _ = tokio::join!(typing_handle, event_collection);

        // Stop everything
        recorder.stop().await.expect("Failed to stop recorder");
        stop_monitoring.store(true, Ordering::Relaxed);
        monitoring_thread.join().expect("Monitoring failed");

        let total_duration = start_time.elapsed();
        let max_memory_kb = memory_samples.load(Ordering::Relaxed);
        let max_cpu_percent = cpu_samples.load(Ordering::Relaxed) as f32 / 100.0;

        // DETAILED PERFORMANCE RESULTS
        println!("\n🎯 TYPING PERFORMANCE & CPU LOAD RESULTS:");
        println!("{}", "=".repeat(80));
        println!("  ⏱️  Total Test Duration: {:?}", total_duration);
        println!("  📊 Events Captured:");
        println!("    📈 Total Events: {}", event_count);
        println!("    ⌨️  Keyboard Events: {}", keyboard_events);
        println!("    📝 Text Completions: {}", text_completion_events);
        println!("  🖥️  System Performance:");
        println!("    🔥 Peak CPU Usage: {:.1}%", max_cpu_percent);
        println!(
            "    🧠 Peak Memory Usage: {} KB ({:.1} MB)",
            max_memory_kb,
            max_memory_kb as f64 / 1024.0
        );

        if keyboard_events > 0 {
            let avg_processing_time = total_keystroke_processing_time / keyboard_events as u32;
            println!("  ⌨️  Keyboard Performance:");
            println!("    ⚡ Average Processing Time: {:?}", avg_processing_time);
            println!(
                "    📊 Total Processing Time: {:?}",
                total_keystroke_processing_time
            );
            println!(
                "    💾 Memory per Keystroke: {:.1} KB",
                max_memory_kb as f64 / keyboard_events as f64
            );

            if total_duration.as_secs_f64() > 0.0 {
                println!(
                    "    🔢 Keystroke Rate: {:.1} keys/sec",
                    keyboard_events as f64 / total_duration.as_secs_f64()
                );
            }
        }

        // Performance analysis and optimization recommendations
        println!("\n📈 TYPING LAG ANALYSIS:");
        if max_cpu_percent > 50.0 {
            println!(
                "🔥 HIGH CPU USAGE: {:.1}% - This could cause typing lag!",
                max_cpu_percent
            );
            println!(
                "💡 RECOMMENDATION: Optimize UI element capture or reduce monitoring frequency"
            );
        } else if max_cpu_percent > 25.0 {
            println!(
                "⚠️ MODERATE CPU USAGE: {:.1}% - Watch for typing lag",
                max_cpu_percent
            );
        } else {
            println!(
                "✅ LOW CPU USAGE: {:.1}% - Good performance",
                max_cpu_percent
            );
        }

        if keyboard_events > 0 {
            let avg_ms = total_keystroke_processing_time.as_millis() / keyboard_events as u128;
            if avg_ms > 10 {
                println!(
                    "🐌 SLOW KEYSTROKE PROCESSING: {}ms average - This causes typing lag!",
                    avg_ms
                );
                println!("💡 RECOMMENDATION: Optimize keyboard event handling pipeline");
            } else if avg_ms > 5 {
                println!(
                    "⚠️ MODERATE processing time: {}ms average per keystroke",
                    avg_ms
                );
            } else {
                println!("✅ FAST keystroke processing: {}ms average", avg_ms);
            }
        }

        let memory_efficiency = if keyboard_events > 0 {
            max_memory_kb as f64 / keyboard_events as f64
        } else {
            0.0
        };
        if memory_efficiency > 1000.0 {
            println!(
                "🧠 HIGH MEMORY per keystroke: {:.1} KB - Consider optimization",
                memory_efficiency
            );
        } else {
            println!(
                "✅ GOOD memory efficiency: {:.1} KB per keystroke",
                memory_efficiency
            );
        }

        // Performance assertions
        assert!(
            total_duration < Duration::from_secs(60),
            "Test should complete within 60 seconds"
        );
        assert!(max_memory_kb < 2_000_000, "Memory should stay under 2GB");

        println!("\n🏁 TYPING PERFORMANCE TEST COMPLETED!");
        println!("💡 Use these metrics to identify and fix typing lag bottlenecks.");
        println!("🔧 Focus optimization on high CPU usage and slow keystroke processing times.");
    }
}

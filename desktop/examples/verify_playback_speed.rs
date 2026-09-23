//! Offline regression check: cargo run --release -p ruffle_desktop --example verify_playback_speed -- C:/Ruffle/6022.swf
use ruffle_core::backend::audio::{
    AudioBackend, AudioMixer, DecodeError, RegisterError, SoundHandle, SoundInstanceHandle,
    SoundStreamInfo, SoundTransform, swf,
};
use ruffle_core::limits::ExecutionLimit;
use ruffle_core::tag_utils::SwfMovie;
use ruffle_core::{FloatDuration, PlayerBuilder, impl_audio_mixer_backend};

struct TestAudio {
    mixer: AudioMixer,
}
impl AudioBackend for TestAudio {
    impl_audio_mixer_backend!(mixer);
    fn play(&mut self) {}
    fn pause(&mut self) {}
    fn set_playback_rate(&mut self, rate: f64) {
        self.mixer.set_playback_rate(rate);
    }
}

fn main() {
    // Test fractional rates and live changes on the SAME playing sound.
    let mut mixer = AudioMixer::new(2, 48000);
    let data: Vec<u8> = (0..44100 * 60)
        .flat_map(|n| {
            let sample =
                ((n as f64 * 440.0 * std::f64::consts::TAU / 44100.0).sin() * 10000.0) as i16;
            sample.to_le_bytes()
        })
        .collect();
    let sound = mixer
        .register_sound(&swf::Sound {
            id: 1,
            format: swf::SoundFormat {
                compression: swf::AudioCompression::Uncompressed,
                sample_rate: 44100,
                is_stereo: false,
                is_16_bit: true,
            },
            num_samples: 44100 * 60,
            data: &data,
        })
        .unwrap();
    let instance = mixer
        .start_sound(
            sound,
            &swf::SoundInfo {
                event: swf::SoundEvent::Event,
                in_sample: None,
                out_sample: None,
                num_loops: 1,
                envelope: None,
            },
        )
        .unwrap();
    let proxy = mixer.proxy();
    let mut output = vec![0.0_f32; 96000];
    for rate in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 1.0] {
        mixer.set_playback_rate(rate);
        let before = mixer.get_sound_position(instance).unwrap();
        proxy.mix(&mut output);
        let elapsed = mixer.get_sound_position(instance).unwrap() - before;
        assert!(
            (elapsed - 1000.0 * rate).abs() < 0.3,
            "{rate}x audio advanced {elapsed}ms"
        );
        assert!(output.iter().any(|x| x.abs() > 0.05));
        println!("PASS live audio {rate}x: {elapsed:.3} movie ms / 1000 wall ms");
    }

    let path = std::env::args().nth(1).expect("Supply a SWF path");
    let data = std::fs::read(&path).unwrap();
    for rate in [0.5, 1.0, 1.5, 2.0, 4.0] {
        let movie = SwfMovie::from_data(
            &data,
            url::Url::from_file_path(&path).unwrap().to_string(),
            None,
            None,
        )
        .unwrap();
        let audio = TestAudio {
            mixer: AudioMixer::new(2, 48000),
        };
        let proxy = audio.mixer.proxy();
        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_audio(audio)
            .with_autoplay(true)
            .build();
        let mut player = player.lock().unwrap();
        player.preload(&mut ExecutionLimit::none());
        player.set_playback_rate(rate);
        assert_eq!(player.playback_rate(), rate);
        let mut nonzero = 0;
        // Equal movie duration at every speed: 20 movie seconds, 5ms wall steps.
        let steps = (4000.0 / rate) as usize;
        let mut buffer = vec![0.0_f32; 480];
        for _ in 0..steps {
            player.tick(FloatDuration::from_millis(5.0));
            proxy.mix(&mut buffer);
            nonzero += buffer.iter().filter(|x| x.abs() > 0.001).count();
        }
        println!(
            "SWF {rate}x: frame={:?}, nonzero_audio_samples={nonzero}",
            player.current_frame()
        );
        assert!(player.current_frame().is_some(), "SWF did not run");
        // 6022.swf is a 12fps timeline: equal movie time must reach the same
        // frame at every rate, allowing one frame for fractional tick rounding.
        if std::path::Path::new(&path).file_name().unwrap() == "6022.swf" {
            assert!(
                (i32::from(player.current_frame().unwrap()) - 240).abs() <= 1,
                "Audio synchronization pulled the movie away from the selected rate"
            );
        }
        assert!(nonzero > 0, "SWF produced no audio");
        let frame = player.current_frame();
        player.set_is_playing(false);
        player.tick(FloatDuration::from_millis(1000.0));
        assert_eq!(player.current_frame(), frame);
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, 5.0] {
            player.set_playback_rate(invalid);
            assert_eq!(player.playback_rate(), rate);
        }
        for target in [721, 121, 1201, 361] {
            player.seek_to_frame(target);
            assert_eq!(player.current_frame(), Some(target));
            assert!(!player.is_playing(), "Seeking a paused movie resumed it");
            assert_eq!(player.playback_rate(), rate);
            player.set_is_playing(true);
            let mut audible = 0;
            for _ in 0..(400.0 / rate) as usize {
                player.tick(FloatDuration::from_millis(5.0));
                proxy.mix(&mut buffer);
                audible += buffer.iter().filter(|x| x.abs() > 0.001).count();
            }
            let end = player.current_frame().unwrap();
            assert!(
                (i32::from(end) - i32::from(target) - 24).abs() <= 1,
                "Seek {target} at {rate}x drifted to frame {end}"
            );
            assert!(audible > 0, "Seek {target} lost audio");
            println!("PASS seek {target} at {rate}x -> frame {end}, audible={audible}");
            player.set_is_playing(false);
        }
    }

    // Compare the actual decoded waveform after a backward seek with the
    // waveform at that same point during uninterrupted playback.
    for target in [121, 361, 721, 1201] {
        let movie = SwfMovie::from_data(
            &data,
            url::Url::from_file_path(&path).unwrap().to_string(),
            None,
            None,
        )
        .unwrap();
        let audio = TestAudio {
            mixer: AudioMixer::new(2, 48000),
        };
        let proxy = audio.mixer.proxy();
        let player = PlayerBuilder::new()
            .with_movie(movie)
            .with_audio(audio)
            .with_autoplay(true)
            .build();
        let mut player = player.lock().unwrap();
        player.preload(&mut ExecutionLimit::none());
        let mut block = vec![0.0_f32; 8000]; // One 12fps frame of stereo output.
        let mut reference = Vec::new();
        for frame in 1..=target + 23 {
            player.run_frame();
            proxy.mix(&mut block);
            if frame >= target {
                reference.extend_from_slice(&block);
            }
        }
        player.seek_to_frame(target);
        let mut sought = Vec::new();
        for frame in target..=target + 23 {
            if frame > target {
                player.run_frame();
            }
            proxy.mix(&mut block);
            sought.extend_from_slice(&block);
        }
        let mut best = (-1.0_f64, 0_i32);
        // Ignore decoder warm-up; MP3 block seeking must align within one SWF frame.
        for lag in -4000_i32..=4000 {
            let (mut xy, mut xx, mut yy) = (0.0_f64, 0.0_f64, 0.0_f64);
            for i in (24000..84000).step_by(128) {
                let x = reference[i * 2] as f64;
                let y = sought[((i as i32 + lag) * 2) as usize] as f64;
                xy += x * y;
                xx += x * x;
                yy += y * y;
            }
            let correlation = xy / (xx * yy).sqrt().max(1e-20);
            if correlation > best.0 {
                best = (correlation, lag);
            }
        }
        println!(
            "Seek frame {target} audio waveform correlation={:.6}, offset={:.3}ms",
            best.0,
            best.1 as f64 / 48.0
        );
        assert!(
            best.0 > 0.95,
            "Seek did not reproduce the audio at the target time"
        );
    }
}

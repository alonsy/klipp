//! Hears "Klipp that". A program of its own so the speech engine and
//! its memory stay out of the recorder, and cost nothing while voice is off.
//!
//! The recorder writes the microphone to stdin, 16 kHz mono 16-bit, and reads
//! one line per thing heard from stdout. End of stdin ends this program, so it
//! can't outlive the recorder.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use sherpa_onnx::{KeywordSpotter, KeywordSpotterConfig, OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig};

const SAMPLE_RATE: i32 = 16_000;
/// 100 ms of audio at a time.
const BLOCK_SAMPLES: usize = 1600;
/// Paths through the keywords kept alive; more is slower and hears no more.
const ACTIVE_PATHS: i32 = 4;
/// Measured on 138 samples: the boost is what wakes up accents, and the
/// threshold barely matters for this model.
const BOOST: f32 = 2.0;
const THRESHOLD: f32 = 0.05;

/// The ways accents say "clip", and "that". "That" changes the most: English
/// `th` is rare in other languages, and many have no `æ`.
const KLIP: [&str; 4] = ["K L IH1 P", "K L IY1 P", "K IH0 L IH1 P", "K AH0 L IH1 P"];
const THAT: [&str; 8] = ["DH AE1 T", "D AE1 T", "D EH1 T", "Z AE1 T", "DH EH1 T", "Z EH1 T", "T AE1 T", "D AH1 T"];

fn keywords(personal: &[String]) -> String {
    let mut lines = BTreeSet::new();
    for klip in KLIP {
        for that in THAT {
            lines.insert(format!("{klip} {that} :{BOOST} #{THRESHOLD} @common"));
        }
    }
    for heard in personal {
        lines.insert(format!("{heard} :{BOOST} #{THRESHOLD} @personal"));
    }
    lines.into_iter().collect::<Vec<_>>().join("\n") + "\n"
}

fn model(dir: &Path) -> OnlineModelConfig {
    let file = |name: &str| Some(dir.join(name).to_string_lossy().into_owned());
    let mut m = OnlineModelConfig::default();
    m.transducer.encoder = file("encoder.int8.onnx");
    m.transducer.decoder = file("decoder.onnx");
    m.transducer.joiner = file("joiner.int8.onnx");
    m.tokens = file("tokens.txt");
    m.num_threads = 1;
    m.provider = Some("cpu".into());
    m
}

/// Fills `block` from stdin; false once stdin has ended.
fn read_block(input: &mut impl Read, bytes: &mut [u8], block: &mut Vec<f32>) -> Result<bool> {
    let mut filled = 0;
    while filled < bytes.len() {
        match input.read(&mut bytes[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    block.clear();
    block.extend(bytes[..filled & !1].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0));
    Ok(filled == bytes.len())
}

/// Prints `hit common|personal PHONEMES` each time it hears the phrase.
fn listen(dir: &Path, personal: &[String]) -> Result<()> {
    let mut config = KeywordSpotterConfig::default();
    config.model_config = model(dir);
    config.max_active_paths = ACTIVE_PATHS;
    config.keywords_buf = Some(keywords(personal));
    let spotter = KeywordSpotter::create(&config).context("the speech model didn't load")?;
    let stream = spotter.create_stream();
    println!("ready");
    std::io::stdout().flush()?;

    let (mut stdin, mut bytes, mut block) = (std::io::stdin().lock(), vec![0u8; BLOCK_SAMPLES * 2], Vec::with_capacity(BLOCK_SAMPLES));
    loop {
        let more = read_block(&mut stdin, &mut bytes, &mut block)?;
        stream.accept_waveform(SAMPLE_RATE, &block);
        while spotter.is_ready(&stream) {
            spotter.decode(&stream);
            if let Some(result) = spotter.get_result(&stream).filter(|r| !r.keyword.is_empty()) {
                println!("hit {} {}", result.keyword, result.tokens_arr.join(" "));
                std::io::stdout().flush()?;
                spotter.reset(&stream);
            }
        }
        if !more {
            return Ok(());
        }
    }
}

/// Prints the phonemes the model hears in stdin, for teaching Klipp a voice.
fn decode(dir: &Path) -> Result<()> {
    let mut config = OnlineRecognizerConfig::default();
    config.model_config = model(dir);
    config.decoding_method = Some("greedy_search".into());
    let recognizer = OnlineRecognizer::create(&config).context("the speech model didn't load")?;
    let stream = recognizer.create_stream();
    let mut pcm = Vec::new();
    std::io::stdin().read_to_end(&mut pcm)?;
    let mut samples: Vec<f32> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    // The model holds back its last moments until more audio pushes them out.
    samples.resize(samples.len() + SAMPLE_RATE as usize, 0.0);
    stream.accept_waveform(SAMPLE_RATE, &samples);
    stream.input_finished();
    while recognizer.is_ready(&stream) {
        recognizer.decode(&stream);
    }
    let tokens = recognizer.get_result(&stream).map(|r| r.tokens).unwrap_or_default();
    println!("{}", tokens.join(" "));
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [mode, dir, rest @ ..] = args.as_slice() else { bail!("usage: klipp-voice listen|decode MODEL_DIR [PERSONAL...]") };
    let dir = Path::new(dir);
    match mode.as_str() {
        "listen" => listen(dir, rest),
        "decode" => decode(dir),
        other => bail!("unknown mode {other}"),
    }
}

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read};
use anyhow::{Result, Context, bail};
use dicom_object::OpenFileOptions;
use dicom_core::{Tag, header::Header, dictionary::DataDictionary};
use dicom_dictionary_std::tags;
use walkdir::WalkDir;
use zip::ZipArchive;
use rayon::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "dicom-json")]
#[command(about = "Advanced DICOM to JSON converter with comprehensive metadata extraction")]
#[command(version = "1.0.0")]
struct Cli {
    /// Input path: DICOM file, directory, or ZIP archive
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    /// Output directory (defaults to current directory)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Output format
    #[arg(short, long, default_value = "comprehensive")]
    format: OutputFormat,

    /// Pretty print JSON output
    #[arg(short, long)]
    pretty: bool,

    /// Process files in parallel (faster for large datasets)
    #[arg(long)]
    parallel: bool,

    /// Include private tags in output
    #[arg(long)]
    include_private: bool,

    /// Organize output by study/series hierarchy
    #[arg(long)]
    organize_hierarchy: bool,

    /// Maximum recursion depth for directory processing
    #[arg(long, default_value = "10")]
    max_depth: usize,

    /// Verbose output
    #[arg(short, long)]
    verbose: bool,
}

#[derive(ValueEnum, Clone, Debug)]
enum OutputFormat {
    /// Basic tag extraction
    Basic,
    /// Comprehensive metadata with descriptions
    Comprehensive,
    /// Structured medical format
    Medical,
    /// Raw DICOM format
    Raw,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DicomStudy {
    pub study_instance_uid: String,
    pub study_date: Option<String>,
    pub study_time: Option<String>,
    pub study_description: Option<String>,
    pub patient_info: PatientInfo,
    pub series: HashMap<String, DicomSeries>,
    pub processing_info: ProcessingInfo,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DicomSeries {
    pub series_instance_uid: String,
    pub series_number: Option<String>,
    pub series_description: Option<String>,
    pub modality: Option<String>,
    pub instances: Vec<DicomInstance>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DicomInstance {
    pub sop_instance_uid: String,
    pub instance_number: Option<String>,
    pub file_path: String,
    pub metadata: DicomMetadata,
    pub has_pixel_data: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PatientInfo {
    pub patient_id: Option<String>,
    pub patient_name: Option<String>,
    pub patient_birth_date: Option<String>,
    pub patient_sex: Option<String>,
    pub patient_age: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DicomMetadata {
    pub tags: HashMap<String, TagInfo>,
    pub transfer_syntax: Option<String>,
    pub sop_class_uid: Option<String>,
    pub file_meta_information: HashMap<String, TagInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TagInfo {
    pub tag: String,
    pub vr: String,
    pub name: Option<String>,
    pub value: serde_json::Value,
    pub raw_value: Option<String>,
    pub is_private: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ProcessingInfo {
    pub processing_id: String,
    pub timestamp: DateTime<Utc>,
    pub version: String,
    pub total_files: usize,
    pub successful_files: usize,
    pub failed_files: usize,
    pub extraction_summary: ExtractionSummary,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExtractionSummary {
    pub files_with_pixel_data: usize,
    pub unique_modalities: Vec<String>,
    pub date_range: Option<(String, String)>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.verbose {
        println!("🏥 Advanced DICOM-JSON Converter v1.0.0");
        println!("📁 Processing: {:?}", cli.input);
    }

    let output_dir = cli.output.as_ref()
        .map(|p| p.clone())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    fs::create_dir_all(&output_dir)?;

    let files = collect_dicom_files(&cli.input, cli.max_depth, cli.verbose)?;
    
    if files.is_empty() {
        bail!("No DICOM files found in the specified input");
    }

    if cli.verbose {
        println!("📊 Found {} DICOM files to process", files.len());
    }

    let progress_bar = if cli.verbose {
        let pb = ProgressBar::new(files.len() as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta})")
            .unwrap()
            .progress_chars("#>-"));
        Some(pb)
    } else {
        None
    };

    let processor = DicomProcessor::new(cli);
    let results = if processor.cli.parallel && files.len() > 1 {
        process_files_parallel(&processor, files, &progress_bar)?
    } else {
        process_files_sequential(&processor, files, &progress_bar)?
    };

    if let Some(pb) = &progress_bar {
        pb.finish_with_message("✅ Processing complete!");
    }

    if processor.cli.organize_hierarchy {
        organize_by_hierarchy(&results, &output_dir, &processor)?;
    } else {
        save_results(&results, &output_dir, &processor)?;
    }

    if processor.cli.verbose {
        print_summary(&results);
    }

    Ok(())
}

struct DicomProcessor {
    cli: Cli,
}

impl DicomProcessor {
    fn new(cli: Cli) -> Self {
        Self { cli }
    }

    fn process_file(&self, file_path: &Path) -> Result<DicomInstance> {
        let obj = OpenFileOptions::new()
            .open_file(file_path)
            .with_context(|| format!("Failed to open DICOM file: {:?}", file_path))?;

        let mut metadata = DicomMetadata {
            tags: HashMap::new(),
            transfer_syntax: None,
            sop_class_uid: None,
            file_meta_information: HashMap::new(),
        };

        // Process file meta information
        let meta = obj.meta();
        metadata.transfer_syntax = Some(meta.transfer_syntax.to_string());

        // Process main dataset
        for element in obj.iter() {
            // Skip private tags if not requested
            if !self.cli.include_private && element.tag().group() % 2 == 1 {
                continue;
            }

            let tag_info = self.create_tag_info(element)?;
            let tag_string = format!("({:04X},{:04X})", element.tag().group(), element.tag().element());
            metadata.tags.insert(tag_string, tag_info);

            // Capture SOP Class UID
            if element.tag() == tags::SOP_CLASS_UID {
                if let Ok(sop) = element.to_str() {
                    metadata.sop_class_uid = Some(sop.to_string());
                }
            }
        }

        let has_pixel_data = obj.element_opt(tags::PIXEL_DATA).is_ok();

        let sop_instance_uid = obj.element_opt(tags::SOP_INSTANCE_UID)
            .ok()
            .flatten()
            .and_then(|elem| elem.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let instance_number = obj.element_opt(tags::INSTANCE_NUMBER)
            .ok()
            .flatten()
            .and_then(|elem| elem.to_str().ok())
            .map(|s| s.to_string());

        Ok(DicomInstance {
            sop_instance_uid,
            instance_number,
            file_path: file_path.to_string_lossy().to_string(),
            metadata,
            has_pixel_data,
        })
    }

    fn create_tag_info(&self, element: &dicom_core::DataElement<dicom_object::InMemDicomObject>) -> Result<TagInfo> {
        let tag = element.tag();
        let vr = element.vr().to_string();
        let tag_string = format!("({:04X},{:04X})", tag.group(), tag.element());
        
        // Get human-readable name from dictionary based on format
        let name = match self.cli.format {
            OutputFormat::Basic | OutputFormat::Raw => None, // No names for basic/raw
            OutputFormat::Comprehensive | OutputFormat::Medical => {
                dicom_dictionary_std::StandardDataDictionary
                    .by_tag(tag)
                    .map(|entry| entry.alias.to_string())
            }
        };

        let is_private = tag.group() % 2 == 1;
        
        let (value, raw_value) = self.extract_element_value(element)?;

        Ok(TagInfo {
            tag: tag_string,
            vr: vr.to_string(),
            name,
            value,
            raw_value,
            is_private,
        })
    }

    fn extract_element_value(&self, element: &dicom_core::DataElement<dicom_object::InMemDicomObject>) -> Result<(serde_json::Value, Option<String>)> {
        match self.cli.format {
            OutputFormat::Raw => {
                let raw = format!("{:?}", element.value());
                Ok((serde_json::Value::String(raw.clone()), Some(raw)))
            }
            _ => {
                let value = match element.to_str() {
                    Ok(string_val) => serde_json::Value::String(string_val.to_string()),
                    Err(_) => {
                        match element.value() {
                            dicom_core::value::Value::Primitive(primitive) => {
                                self.convert_primitive_value(primitive)?
                            },
                            dicom_core::value::Value::Sequence(seq) => {
                                serde_json::Value::Array(
                                    (0..seq.length().get().unwrap_or(0))
                                        .map(|i| serde_json::Value::String(format!("Sequence Item {}", i + 1)))
                                        .collect()
                                )
                            },
                            _ => serde_json::Value::String(format!("{:?}", element.value())),
                        }
                    }
                };
                
                let raw = element.to_str().ok().map(|s| s.to_string());
                Ok((value, raw))
            }
        }
    }

    fn convert_primitive_value(&self, primitive: &dicom_core::value::PrimitiveValue) -> Result<serde_json::Value> {
        use dicom_core::value::PrimitiveValue::*;
        
        let value = match primitive {
            U8(vals) => {
                if vals.len() == 1 {
                    serde_json::Value::Number(vals[0].into())
                } else {
                    serde_json::Value::Array(vals.iter().map(|&v| serde_json::Value::Number(v.into())).collect())
                }
            },
            U16(vals) => {
                if vals.len() == 1 {
                    serde_json::Value::Number(vals[0].into())
                } else {
                    serde_json::Value::Array(vals.iter().map(|&v| serde_json::Value::Number(v.into())).collect())
                }
            },
            U32(vals) => {
                if vals.len() == 1 {
                    serde_json::Value::Number(vals[0].into())
                } else {
                    serde_json::Value::Array(vals.iter().map(|&v| serde_json::Value::Number(v.into())).collect())
                }
            },
            I16(vals) => {
                if vals.len() == 1 {
                    serde_json::Value::Number(vals[0].into())
                } else {
                    serde_json::Value::Array(vals.iter().map(|&v| serde_json::Value::Number(v.into())).collect())
                }
            },
            I32(vals) => {
                if vals.len() == 1 {
                    serde_json::Value::Number(vals[0].into())
                } else {
                    serde_json::Value::Array(vals.iter().map(|&v| serde_json::Value::Number(v.into())).collect())
                }
            },
            F32(vals) => {
                if vals.len() == 1 {
                    serde_json::json!(vals[0])
                } else {
                    serde_json::Value::Array(vals.iter().map(|&v| serde_json::json!(v)).collect())
                }
            },
            F64(vals) => {
                if vals.len() == 1 {
                    serde_json::json!(vals[0])
                } else {
                    serde_json::Value::Array(vals.iter().map(|&v| serde_json::json!(v)).collect())
                }
            },
            Str(s) => serde_json::Value::String(s.to_string()),
            Strs(strs) => {
                if strs.len() == 1 {
                    serde_json::Value::String(strs[0].to_string())
                } else {
                    serde_json::Value::Array(strs.iter().map(|s| serde_json::Value::String(s.to_string())).collect())
                }
            },
            Tags(tags) => {
                serde_json::Value::Array(
                    tags.iter().map(|tag| {
                        serde_json::Value::String(format!("({:04X},{:04X})", tag.group(), tag.element()))
                    }).collect()
                )
            },
            Date(dates) => {
                if dates.len() == 1 {
                    serde_json::Value::String(dates[0].to_string())
                } else {
                    serde_json::Value::Array(dates.iter().map(|d| serde_json::Value::String(d.to_string())).collect())
                }
            },
            Time(times) => {
                if times.len() == 1 {
                    serde_json::Value::String(times[0].to_string())
                } else {
                    serde_json::Value::Array(times.iter().map(|t| serde_json::Value::String(t.to_string())).collect())
                }
            },
            DateTime(datetimes) => {
                if datetimes.len() == 1 {
                    serde_json::Value::String(datetimes[0].to_string())
                } else {
                    serde_json::Value::Array(datetimes.iter().map(|dt| serde_json::Value::String(dt.to_string())).collect())
                }
            },
            _ => serde_json::Value::String(format!("{:?}", primitive)),
        };
        
        Ok(value)
    }
}

fn collect_dicom_files(input: &Path, max_depth: usize, verbose: bool) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if input.is_file() {
        if let Some(ext) = input.extension() {
            if ext.eq_ignore_ascii_case("zip") {
                // Handle ZIP file
                if verbose {
                    println!("📦 Extracting ZIP archive...");
                }
                files.extend(extract_zip_files(input)?);
            } else {
                files.push(input.to_path_buf());
            }
        } else {
            files.push(input.to_path_buf());
        }
    } else if input.is_dir() {
        // Walk directory
        for entry in WalkDir::new(input)
            .max_depth(max_depth)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                let path = entry.path();
                if is_likely_dicom_file(path) {
                    files.push(path.to_path_buf());
                }
            }
        }
    } else {
        bail!("Input path does not exist: {:?}", input);
    }

    Ok(files)
}

fn extract_zip_files(zip_path: &Path) -> Result<Vec<PathBuf>> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(BufReader::new(file))?;
    let mut extracted_files = Vec::new();

    let temp_dir = std::env::temp_dir().join(format!("dicom_extract_{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_dir)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        if !file.is_dir() {
            let file_path = temp_dir.join(file.name());
            
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut output = File::create(&file_path)?;
            std::io::copy(&mut file, &mut output)?;

            if is_likely_dicom_file(&file_path) {
                extracted_files.push(file_path);
            }
        }
    }

    Ok(extracted_files)
}

fn is_likely_dicom_file(path: &Path) -> bool {
    // Check file extension
    if let Some(ext) = path.extension() {
        let ext_str = ext.to_string_lossy().to_lowercase();
        if matches!(ext_str.as_str(), "dcm" | "dicom" | "ima" | "img") {
            return true;
        }
    }

    // Check for DICOM magic bytes
    if let Ok(mut file) = File::open(path) {
        let mut buffer = [0u8; 132];
        if file.read_exact(&mut buffer).is_ok() {
            return &buffer[128..132] == b"DICM";
        }
    }

    false
}

fn process_files_sequential(
    processor: &DicomProcessor, 
    files: Vec<PathBuf>, 
    progress_bar: &Option<ProgressBar>
) -> Result<Vec<DicomInstance>> {
    let mut results = Vec::new();
    
    for file in files {
        if let Some(pb) = progress_bar {
            pb.set_message(format!("Processing: {}", file.file_name().unwrap_or_default().to_string_lossy()));
        }

        match processor.process_file(&file) {
            Ok(instance) => results.push(instance),
            Err(e) => {
                if processor.cli.verbose {
                    eprintln!("❌ Failed to process {:?}: {}", file, e);
                }
            }
        }

        if let Some(pb) = progress_bar {
            pb.inc(1);
        }
    }

    Ok(results)
}

fn process_files_parallel(
    processor: &DicomProcessor, 
    files: Vec<PathBuf>, 
    progress_bar: &Option<ProgressBar>
) -> Result<Vec<DicomInstance>> {
    let results: Vec<_> = files
        .par_iter()
        .filter_map(|file| {
            let result = processor.process_file(file);
            if let Some(pb) = progress_bar {
                pb.inc(1);
            }
            match result {
                Ok(instance) => Some(instance),
                Err(e) => {
                    if processor.cli.verbose {
                        eprintln!("❌ Failed to process {:?}: {}", file, e);
                    }
                    None
                }
            }
        })
        .collect();

    Ok(results)
}

fn organize_by_hierarchy(
    results: &[DicomInstance], 
    output_dir: &Path, 
    processor: &DicomProcessor
) -> Result<()> {
    let mut studies: HashMap<String, DicomStudy> = HashMap::new();

    for instance in results {
        let study_uid = get_tag_value(&instance.metadata.tags, tags::STUDY_INSTANCE_UID)
            .unwrap_or_else(|| "unknown_study".to_string());
        
        let series_uid = get_tag_value(&instance.metadata.tags, tags::SERIES_INSTANCE_UID)
            .unwrap_or_else(|| "unknown_series".to_string());

        let study = studies.entry(study_uid.clone()).or_insert_with(|| {
            DicomStudy {
                study_instance_uid: study_uid.clone(),
                study_date: get_tag_value(&instance.metadata.tags, tags::STUDY_DATE),
                study_time: get_tag_value(&instance.metadata.tags, tags::STUDY_TIME),
                study_description: get_tag_value(&instance.metadata.tags, tags::STUDY_DESCRIPTION),
                patient_info: extract_patient_info(&instance.metadata.tags),
                series: HashMap::new(),
                processing_info: ProcessingInfo {
                    processing_id: Uuid::new_v4().to_string(),
                    timestamp: Utc::now(),
                    version: "1.0.0".to_string(),
                    total_files: results.len(),
                    successful_files: results.len(),
                    failed_files: 0,
                    extraction_summary: ExtractionSummary {
                        files_with_pixel_data: results.iter().filter(|r| r.has_pixel_data).count(),
                        unique_modalities: results.iter()
                            .filter_map(|r| get_tag_value(&r.metadata.tags, tags::MODALITY))
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .collect(),
                        date_range: None,
                    },
                },
            }
        });

        let series = study.series.entry(series_uid.clone()).or_insert_with(|| {
            DicomSeries {
                series_instance_uid: series_uid,
                series_number: get_tag_value(&instance.metadata.tags, tags::SERIES_NUMBER),
                series_description: get_tag_value(&instance.metadata.tags, tags::SERIES_DESCRIPTION),
                modality: get_tag_value(&instance.metadata.tags, tags::MODALITY),
                instances: Vec::new(),
            }
        });

        series.instances.push(instance.clone());
    }

    // Save organized studies
    for (study_uid, study) in studies {
        let study_dir = output_dir.join(format!("study_{}", sanitize_filename(&study_uid)));
        fs::create_dir_all(&study_dir)?;

        let study_output = match processor.cli.format {
            OutputFormat::Basic => create_basic_study_output(&study),
            OutputFormat::Medical => create_medical_study_output(&study),
            OutputFormat::Raw => create_raw_study_output(&study),
            OutputFormat::Comprehensive => serde_json::to_value(&study)?,
        };

        let json_content = if processor.cli.pretty {
            serde_json::to_string_pretty(&study_output)?
        } else {
            serde_json::to_string(&study_output)?
        };

        let json_file = study_dir.join("study.json");
        fs::write(json_file, json_content)?;

        if processor.cli.verbose {
            println!("📄 Study saved: {:?}/study.json", study_dir);
        }
    }

    Ok(())
}

fn save_results(
    results: &[DicomInstance], 
    output_dir: &Path, 
    processor: &DicomProcessor
) -> Result<()> {
    let output_data = match processor.cli.format {
        OutputFormat::Basic => create_basic_output(results),
        OutputFormat::Comprehensive => create_comprehensive_output(results),
        OutputFormat::Medical => create_medical_output(results),
        OutputFormat::Raw => create_raw_output(results),
    };

    let json_content = if processor.cli.pretty {
        serde_json::to_string_pretty(&output_data)?
    } else {
        serde_json::to_string(&output_data)?
    };

    let output_file = output_dir.join("dicom_data.json");
    fs::write(&output_file, json_content)?;

    if processor.cli.verbose {
        println!("📄 Results saved to: {:?}", output_file);
    }

    Ok(())
}

fn create_basic_output(results: &[DicomInstance]) -> serde_json::Value {
    let basic_instances: Vec<_> = results.iter().map(|instance| {
        serde_json::json!({
            "file_path": instance.file_path,
            "sop_instance_uid": instance.sop_instance_uid,
            "tags": instance.metadata.tags.iter()
                .filter(|(_, tag_info)| !tag_info.is_private)
                .take(10) // Limit to first 10 tags for basic format
                .map(|(k, v)| (k.clone(), v.value.clone()))
                .collect::<serde_json::Map<String, serde_json::Value>>()
        })
    }).collect();

    serde_json::json!({
        "format": "basic",
        "total_files": results.len(),
        "instances": basic_instances
    })
}

fn create_comprehensive_output(results: &[DicomInstance]) -> serde_json::Value {
    let processing_info = ProcessingInfo {
        processing_id: Uuid::new_v4().to_string(),
        timestamp: Utc::now(),
        version: "1.0.0".to_string(),
        total_files: results.len(),
        successful_files: results.len(),
        failed_files: 0,
        extraction_summary: ExtractionSummary {
            files_with_pixel_data: results.iter().filter(|r| r.has_pixel_data).count(),
            unique_modalities: results.iter()
                .filter_map(|r| get_tag_value(&r.metadata.tags, tags::MODALITY))
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect(),
            date_range: None,
        },
    };

    serde_json::json!({
        "format": "comprehensive",
        "processing_info": processing_info,
        "instances": results
    })
}

fn create_medical_output(results: &[DicomInstance]) -> serde_json::Value {
    let medical_instances: Vec<_> = results.iter().map(|instance| {
        serde_json::json!({
            "file_path": instance.file_path,
            "patient": {
                "id": get_tag_value(&instance.metadata.tags, tags::PATIENT_ID),
                "name": get_tag_value(&instance.metadata.tags, tags::PATIENT_NAME),
                "birth_date": get_tag_value(&instance.metadata.tags, tags::PATIENT_BIRTH_DATE),
                "sex": get_tag_value(&instance.metadata.tags, tags::PATIENT_SEX),
                "age": get_tag_value(&instance.metadata.tags, tags::PATIENT_AGE),
            },
            "study": {
                "uid": get_tag_value(&instance.metadata.tags, tags::STUDY_INSTANCE_UID),
                "date": get_tag_value(&instance.metadata.tags, tags::STUDY_DATE),
                "time": get_tag_value(&instance.metadata.tags, tags::STUDY_TIME),
                "description": get_tag_value(&instance.metadata.tags, tags::STUDY_DESCRIPTION),
            },
            "series": {
                "uid": get_tag_value(&instance.metadata.tags, tags::SERIES_INSTANCE_UID),
                "number": get_tag_value(&instance.metadata.tags, tags::SERIES_NUMBER),
                "description": get_tag_value(&instance.metadata.tags, tags::SERIES_DESCRIPTION),
                "modality": get_tag_value(&instance.metadata.tags, tags::MODALITY),
            },
            "instance": {
                "uid": instance.sop_instance_uid.clone(),
                "number": instance.instance_number.clone(),
                "has_pixel_data": instance.has_pixel_data,
            },
            "imaging": {
                "rows": get_tag_value(&instance.metadata.tags, tags::ROWS),
                "columns": get_tag_value(&instance.metadata.tags, tags::COLUMNS),
                "bits_allocated": get_tag_value(&instance.metadata.tags, tags::BITS_ALLOCATED),
                "photometric_interpretation": get_tag_value(&instance.metadata.tags, tags::PHOTOMETRIC_INTERPRETATION),
                "transfer_syntax": instance.metadata.transfer_syntax.clone(),
            }
        })
    }).collect();

    serde_json::json!({
        "format": "medical",
        "summary": {
            "total_instances": results.len(),
            "files_with_images": results.iter().filter(|r| r.has_pixel_data).count(),
            "unique_modalities": results.iter()
                .filter_map(|r| get_tag_value(&r.metadata.tags, tags::MODALITY))
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect::<Vec<_>>(),
        },
        "instances": medical_instances
    })
}

fn create_raw_output(results: &[DicomInstance]) -> serde_json::Value {
    let raw_instances: Vec<_> = results.iter().map(|instance| {
        serde_json::json!({
            "file": instance.file_path,
            "tags": instance.metadata.tags.iter()
                .map(|(k, v)| (k.clone(), serde_json::json!({
                    "vr": v.vr,
                    "raw": v.raw_value,
                    "private": v.is_private
                })))
                .collect::<serde_json::Map<String, serde_json::Value>>()
        })
    }).collect();

    serde_json::json!({
        "format": "raw",
        "instances": raw_instances
    })
}

fn create_basic_study_output(study: &DicomStudy) -> serde_json::Value {
    serde_json::json!({
        "format": "basic",
        "study_uid": study.study_instance_uid,
        "study_date": study.study_date,
        "series_count": study.series.len(),
        "total_instances": study.series.values().map(|s| s.instances.len()).sum::<usize>(),
        "modalities": study.series.values()
            .filter_map(|s| s.modality.as_ref())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    })
}

fn create_medical_study_output(study: &DicomStudy) -> serde_json::Value {
    let series_summary: Vec<_> = study.series.values().map(|series| {
        serde_json::json!({
            "uid": series.series_instance_uid,
            "number": series.series_number,
            "description": series.series_description,
            "modality": series.modality,
            "instance_count": series.instances.len(),
            "has_images": series.instances.iter().any(|i| i.has_pixel_data)
        })
    }).collect();

    serde_json::json!({
        "format": "medical",
        "study": {
            "uid": study.study_instance_uid,
            "date": study.study_date,
            "time": study.study_time,
            "description": study.study_description
        },
        "patient": study.patient_info,
        "series": series_summary,
        "summary": {
            "total_series": study.series.len(),
            "total_instances": study.series.values().map(|s| s.instances.len()).sum::<usize>(),
            "imaging_instances": study.series.values()
                .flat_map(|s| &s.instances)
                .filter(|i| i.has_pixel_data)
                .count()
        }
    })
}

fn create_raw_study_output(study: &DicomStudy) -> serde_json::Value {
    serde_json::json!({
        "format": "raw",
        "study_uid": study.study_instance_uid,
        "files": study.series.values()
            .flat_map(|s| &s.instances)
            .map(|i| i.file_path.clone())
            .collect::<Vec<_>>(),
        "tag_count": study.series.values()
            .flat_map(|s| &s.instances)
            .map(|i| i.metadata.tags.len())
            .sum::<usize>()
    })
}

fn get_tag_value(tags: &HashMap<String, TagInfo>, tag: Tag) -> Option<String> {
    let tag_string = format!("({:04X},{:04X})", tag.group(), tag.element());
    tags.get(&tag_string)?.raw_value.clone()
}

fn extract_patient_info(tags: &HashMap<String, TagInfo>) -> PatientInfo {
    PatientInfo {
        patient_id: get_tag_value(tags, tags::PATIENT_ID),
        patient_name: get_tag_value(tags, tags::PATIENT_NAME),
        patient_birth_date: get_tag_value(tags, tags::PATIENT_BIRTH_DATE),
        patient_sex: get_tag_value(tags, tags::PATIENT_SEX),
        patient_age: get_tag_value(tags, tags::PATIENT_AGE),
    }
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn print_summary(results: &[DicomInstance]) {
    println!("\nProcessing Summary:");
    println!("   Total instances: {}", results.len());
    println!("   Files with pixel data: {}", results.iter().filter(|r| r.has_pixel_data).count());
    
    let modalities: std::collections::HashSet<_> = results.iter()
        .filter_map(|r| get_tag_value(&r.metadata.tags, tags::MODALITY))
        .collect();
    println!("   Unique modalities: {}", modalities.len());
    
    for modality in &modalities {
        println!("     - {}", modality);
    }
}
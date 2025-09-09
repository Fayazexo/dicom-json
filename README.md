# DICOM-JSON Converter

**Convert DICOM medical files to JSON format**

Simple CLI tool that extracts metadata from DICOM files and outputs structured JSON. Supports single files, directories, ZIP archives, and multiple output formats.

## ⚡ Quick Install

**Linux/macOS:**

```bash
curl -sSL https://raw.githubusercontent.com/fayazexo/dicom-json/main/install.sh | bash
```

**Windows (PowerShell):**

```powershell
iwr -useb https://raw.githubusercontent.com/fayazexo/dicom-json/main/install.ps1 | iex
```

**Alternative: Download Binary**

- Go to [Releases](https://github.com/fayazexo/dicom-json/releases)
- Download for your platform
- Extract and add to PATH

## Usage

```bash
# Convert single file
dicom-json input.dcm

# Pretty print
dicom-json input.dcm --pretty

# Process directory
dicom-json /path/to/dicom/files/ --verbose

# Different output formats
dicom-json input.dcm --format medical --pretty
dicom-json input.dcm --format basic
dicom-json input.dcm --format raw --include-private

# Organize by medical hierarchy
dicom-json study.zip --organize-hierarchy --output ./results/
```

## Output Formats

- **`comprehensive`** (default) - Full metadata with human-readable names
- **`medical`** - Structured for clinical use (Patient→Study→Series→Instance)
- **`basic`** - Minimal output, first 10 tags only
- **`raw`** - Technical DICOM debugging format

## Options

```
dicom-json [OPTIONS] <INPUT>

Arguments:
  <INPUT>  DICOM file, directory, or ZIP archive

Options:
  -f, --format <FORMAT>     Output format [default: comprehensive]
  -o, --output <OUTPUT>     Output directory
  -p, --pretty              Pretty print JSON
      --organize-hierarchy  Group by study/series structure
      --include-private     Include private DICOM tags
      --parallel            Process files in parallel
  -v, --verbose             Show progress and details
  -h, --help                Show help
```

## Examples

### Basic Conversion

```bash
dicom-json scan.dcm --pretty
```

### Medical Workflow

```bash
dicom-json patient_study/ --format medical --organize-hierarchy --verbose
```

### Batch Processing

```bash
dicom-json large_dataset.zip --parallel --output ./processed/
```

## Requirements

- No dependencies needed - single binary
- Works with all DICOM files (CT, MR, X-Ray, etc.)
- Cross-platform (Windows, macOS, Linux)

## Help

Having issues? [Open an issue](https://github.com/fayazexo/dicom-json/issues) or check the [discussions](https://github.com/fayazexo/dicom-json/discussions).

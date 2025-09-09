# Simple DICOM-JSON Windows Installer
# Downloads the binary and makes it available globally

param(
    [switch]$AddToPath = $true
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$Repo = "fayazexo/dicom-json"
$InstallDir = "$env:LOCALAPPDATA\Programs\dicom-json"

Write-Host "🏥 Installing DICOM-JSON..." -ForegroundColor Blue

# Create install directory
New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null

# Get latest version
Write-Host "📡 Getting latest version..." -ForegroundColor White
try {
    $Response = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $Response.tag_name
    Write-Host "📦 Latest version: $Version" -ForegroundColor White
}
catch {
    Write-Host "❌ Failed to get latest version: $_" -ForegroundColor Red
    exit 1
}

# Download
$Filename = "dicom-json-windows-x86_64.zip"
$Url = "https://github.com/$Repo/releases/download/$Version/$Filename"
$TempFile = Join-Path $env:TEMP $Filename

Write-Host "⬇️ Downloading $Filename..." -ForegroundColor White

try {
    Invoke-WebRequest -Uri $Url -OutFile $TempFile -UseBasicParsing
    
    # Extract
    Expand-Archive -Path $TempFile -DestinationPath $InstallDir -Force
    
    # Clean up
    Remove-Item $TempFile -Force
    
    Write-Host "✅ dicom-json installed to $InstallDir" -ForegroundColor Green
}
catch {
    Write-Host "❌ Download failed: $_" -ForegroundColor Red
    exit 1
}

# Add to PATH
if ($AddToPath) {
    $CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
    
    if ($CurrentPath -like "*$InstallDir*") {
        Write-Host "🔧 Already in PATH" -ForegroundColor Yellow
    }
    else {
        Write-Host "🔧 Adding to PATH..." -ForegroundColor White
        $NewPath = "$CurrentPath;$InstallDir"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        $env:Path += ";$InstallDir"
        Write-Host "✅ Added to PATH" -ForegroundColor Green
    }
    
    Write-Host ""
    Write-Host "🎉 Installation complete! Try: dicom-json --help" -ForegroundColor Green
}
else {
    Write-Host ""
    Write-Host "✅ Installation complete!" -ForegroundColor Green
    Write-Host "Run: $InstallDir\dicom-json.exe --help" -ForegroundColor White
}
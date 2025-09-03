use std::{
    collections::HashMap,
    fs,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use alloy_json_abi::JsonAbi;
use anyhow::Context;
use csv::Writer;
use foundry_evm_core::abi::{TestFunctionExt, TestFunctionKind};
use git2::Repository;
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use walkdir::WalkDir;

#[derive(Debug)]
struct FoundryConfig {
    /// Test directories (different profiles can have different test
    /// directories)
    test: Vec<String>,
    build_profiles: Vec<BuildProfile>,
    has_eth_rpc_url: bool,
}

#[derive(Debug)]
struct BuildProfile {
    /// Artifact output directory
    out: String,
    /// Profile name
    profile: String,
}

#[derive(Deserialize)]
struct Artifact<'a> {
    abi: JsonAbi,
    #[serde(borrow)]
    bytecode: Bytecode<'a>,
}

#[derive(Deserialize)]
struct Bytecode<'a> {
    object: &'a str,
}

#[derive(Debug, Clone)]
struct RepoAnalysis {
    repo_url: String,
    commit: String,
    test_kinds: HashMap<TestFunctionKind, bool>,
    global_fork: bool,
    cheatcode_fork: bool,
}

#[derive(Debug, Clone)]
struct GitHubUrl {
    base_url: String,
    org: String,
    repo: String,
    branch: Option<String>,
    subdirectory: Option<String>,
}

#[derive(Debug, Serialize)]
struct CsvRecord {
    repo_url: String,
    commit: String,
    /// Whether the repo uses the `setup` method for test contracts
    setup: bool,
    unit_test: bool,
    /// Whether there are any unit test with the `testFail` prefix.
    unit_test_fail: bool,
    fuzz_test: bool,
    /// Whether there are any fuzz test with the `testFail` prefix.
    fuzz_test_fail: bool,
    invariant_test: bool,
    /// Foundry v1.3.0 comes with support for table testing, which enables the
    /// definition of a dataset (the "table") and the execution of a test
    /// function for each entry in that dataset.
    /// <https://getfoundry.sh/forge/advanced-testing/table-testing#table-testing>
    table_test: bool,
    /// afterInvariant() function is called at the end of each invariant run (if
    /// declared), allowing post campaign processing
    /// <https://getfoundry.sh/forge/advanced-testing/invariant-testing#overview>
    after_invariant: bool,
    /// Whether the repo uses the `eth_rpc_url` global fork option in
    /// foundry.toml
    global_fork: bool,
    /// Whether the repo uses the createFork or createSelectFork cheatcodes
    cheatcode_fork: bool,
}

impl From<RepoAnalysis> for CsvRecord {
    fn from(analysis: RepoAnalysis) -> Self {
        Self {
            repo_url: analysis.repo_url,
            commit: analysis.commit,
            setup: analysis
                .test_kinds
                .get(&TestFunctionKind::Setup)
                .copied()
                .unwrap_or(false),
            unit_test: analysis
                .test_kinds
                .get(&TestFunctionKind::UnitTest { should_fail: false })
                .copied()
                .unwrap_or(false),
            unit_test_fail: analysis
                .test_kinds
                .get(&TestFunctionKind::UnitTest { should_fail: true })
                .copied()
                .unwrap_or(false),
            fuzz_test: analysis
                .test_kinds
                .get(&TestFunctionKind::FuzzTest { should_fail: false })
                .copied()
                .unwrap_or(false),
            fuzz_test_fail: analysis
                .test_kinds
                .get(&TestFunctionKind::FuzzTest { should_fail: true })
                .copied()
                .unwrap_or(false),
            invariant_test: analysis
                .test_kinds
                .get(&TestFunctionKind::InvariantTest)
                .copied()
                .unwrap_or(false),
            table_test: analysis
                .test_kinds
                .get(&TestFunctionKind::TableTest)
                .copied()
                .unwrap_or(false),
            after_invariant: analysis
                .test_kinds
                .get(&TestFunctionKind::AfterInvariant)
                .copied()
                .unwrap_or(false),
            global_fork: analysis.global_fork,
            cheatcode_fork: analysis.cheatcode_fork,
        }
    }
}

pub fn analyze_repos(output_path: &Path) -> anyhow::Result<()> {
    let stdin = io::stdin();
    let reader = BufReader::new(stdin);

    // Collect all repo URLs first
    let repo_urls: Vec<String> = reader
        .lines()
        .filter_map(|line| {
            line.ok()
                .filter(|url| !url.trim().is_empty())
                .map(|url| url.trim().to_string())
        })
        .collect();

    println!("Found {} repositories to analyze", repo_urls.len());

    // Limit parallelism for requests to Github to 4
    let github_semaphore = Arc::new(Semaphore::new(4));

    let handle = tokio::runtime::Handle::current();

    // Process repositories in parallel using rayon
    let analyses: Vec<RepoAnalysis> = repo_urls
        .par_iter()
        .filter_map(|repo_url| {
            let _ = handle.enter();
            match analyze_single_repo(repo_url, github_semaphore.clone()) {
                Ok(analysis) => {
                    println!("✓ Successfully analyzed {}", repo_url);
                    Some(analysis)
                }
                Err(e) => {
                    eprintln!("✗ Failed to analyze {}: {}", repo_url, e);
                    None
                }
            }
        })
        .collect();

    // Write results to CSV
    write_csv_results(&analyses, output_path)?;

    println!(
        "Analysis complete. {} repositories analyzed successfully. Results written to {}",
        analyses.len(),
        output_path.display()
    );
    Ok(())
}

fn parse_github_url(url: &str) -> anyhow::Result<GitHubUrl> {
    // Parse URLs like:
    // - https://github.com/owner/repo
    // - https://github.com/owner/repo/tree/branch/path/to/dir

    let path = url
        .strip_prefix("https://github.com/")
        .with_context(|| "Only GitHub URLs are supported".to_string())?;
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();

    if parts.len() < 2 {
        anyhow::bail!("Invalid GitHub URL format");
    }

    let org = parts[0].to_string();
    let repo = parts[1].to_string();
    let base_url = format!("https://github.com/{}/{}", org, repo);

    if parts.len() == 2 {
        // Simple repo URL
        return Ok(GitHubUrl {
            base_url,
            org,
            repo,
            branch: None,
            subdirectory: None,
        });
    }

    if parts.len() >= 4 && parts[2] == "tree" {
        // URL with branch and possibly subdirectory
        let branch = Some(parts[3].to_string());
        let subdirectory = if parts.len() > 4 {
            Some(parts[4..].join("/"))
        } else {
            None
        };

        return Ok(GitHubUrl {
            base_url,
            org,
            repo,
            branch,
            subdirectory,
        });
    }

    anyhow::bail!("Unsupported GitHub URL format")
}

struct RepoSetup {
    working_dir: PathBuf,
    commit: String,
}

fn setup_repo(repo_url: &str, github_semaphore: Arc<Semaphore>) -> anyhow::Result<RepoSetup> {
    // Limit parallel requests to Github to avoid getting rate limited
    let _ = tokio::runtime::Handle::current().block_on(github_semaphore.acquire())?;

    // Parse the GitHub URL
    let github_url = parse_github_url(repo_url)?;

    // Create repos directory in crates/tools
    let tools_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repos_dir = tools_dir.join("repos");
    fs::create_dir_all(&repos_dir)
        .with_context(|| "Failed to create repos directory".to_string())?;

    // Generate a safe directory name from the parsed org and repo names
    let repo_dir_name = format!("{}_{}", github_url.org, github_url.repo);
    let repo_path = repos_dir.join(&repo_dir_name);

    // Check out repo if it doesn't exist. Not using the `git2` crate for this,
    // because it doesn't support --depth 1 along with recursively cloning
    // submodules.
    if !repo_path.exists() {
        // git clone --recurse-submodules https://github.com/Uniswap/v4-periphery.git --depth 1
        Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--recurse-submodules")
            .arg(&github_url.base_url)
            .arg(&repo_path)
            .output()
            .with_context(|| format!("Failed to check out repo: {}", &github_url.base_url))?;
    }

    // Update repo
    let repo = Repository::open(&repo_path).with_context(|| {
        format!(
            "Failed to open existing repository: {}",
            repo_path.display()
        )
    })?;

    // Fetch latest changes
    {
        let mut remote = repo.find_remote("origin").with_context(|| {
            format!(
                "Failed to find origin remote for repo: {}",
                github_url.base_url
            )
        })?;
        remote
            .fetch(&["refs/heads/*:refs/remotes/origin/*"], None, None)
            .with_context(|| {
                format!(
                    "Failed to fetch from origin for repo: {}",
                    github_url.base_url
                )
            })?;
    }

    // Determine target branch
    let target_branch = if let Some(ref branch) = github_url.branch {
        format!("refs/remotes/origin/{}", branch)
    } else if repo.find_reference("refs/remotes/origin/main").is_ok() {
        "refs/remotes/origin/main".to_string()
    } else {
        "refs/remotes/origin/master".to_string()
    };

    if let Ok(target_ref) = repo.find_reference(&target_branch) {
        if let Some(target_oid) = target_ref.target() {
            repo.reset(
                &repo.find_object(target_oid, None)?,
                git2::ResetType::Hard,
                None,
            )
            .with_context(|| {
                format!(
                    "Failed to reset to latest commit for repo: {}",
                    github_url.base_url
                )
            })?;
        }
    }

    // Get commit hash
    let head = repo.head().with_context(|| {
        format!(
            "Failed to get HEAD reference for repo: {}",
            github_url.base_url
        )
    })?;
    let commit = head.target().with_context(|| {
        format!(
            "Failed to get commit hash for repo: {}",
            github_url.base_url
        )
    })?;
    let commit_str = commit.to_string();

    // Determine working directory (subdirectory if specified)
    let working_dir = if let Some(ref subdir) = github_url.subdirectory {
        repo_path.join(subdir)
    } else {
        repo_path.clone()
    };

    if !working_dir.exists() {
        anyhow::bail!(format!("Subdirectory not found: {}", working_dir.display()))
    }

    // Check for package.json and run pnpm install if found
    let package_json_path = working_dir.join("package.json");
    let makefile_path = working_dir.join("Makefile");
    if makefile_path.exists() {
        Command::new("make")
            .current_dir(&working_dir)
            .output()
            .with_context(|| format!("Failed to run make in: {}", working_dir.display()))?;
    } else if package_json_path.exists() {
        // Make sure the appropriate package manager version is installed
        Command::new("corepack")
            .arg("install")
            .current_dir(&working_dir)
            .output()
            .with_context(|| {
                format!(
                    "Failed to execute corepack install in: {}",
                    working_dir.display()
                )
            })?;

        // Use yarn if there yarn.lock file, otherwise default to pnpm
        let yarn_lock_path = working_dir.join("yarn.lock");
        if yarn_lock_path.exists() {
            Command::new("yarn")
                .current_dir(&working_dir)
                .output()
                .with_context(|| {
                    format!(
                        "Failed to execute yarn install in: {}",
                        working_dir.display()
                    )
                })?;
        } else {
            Command::new("pnpm")
                .arg("install")
                .current_dir(&working_dir)
                .output()
                .with_context(|| {
                    format!(
                        "Failed to execute pnpm install in: {}",
                        working_dir.display()
                    )
                })?;
        }
    }

    Ok(RepoSetup {
        working_dir,
        commit: commit_str,
    })
}

fn analyze_single_repo(
    repo_url: &str,
    github_semaphore: Arc<Semaphore>,
) -> anyhow::Result<RepoAnalysis> {
    let repo_setup = setup_repo(repo_url, github_semaphore)?;

    let foundry_config = get_foundry_toml(&repo_setup.working_dir)?;

    // Run forge build
    for build_profile in foundry_config.build_profiles.iter() {
        let output = Command::new("forge")
            .arg("build")
            .arg("--out")
            .arg(&build_profile.out)
            .current_dir(&repo_setup.working_dir)
            .env("FOUNDRY_PROFILE", &build_profile.profile)
            .output()
            .with_context(|| {
                format!(
                    "Failed to execute forge build in: {}",
                    repo_setup.working_dir.display()
                )
            })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("forge build failed: {}", stderr);
        }
    }

    // Analyze contracts
    let mut test_kinds = HashMap::new();

    let contracts =
        find_test_contract_abis(&repo_setup.working_dir, &foundry_config.build_profiles)?;

    for contract_abi in contracts {
        analyze_contract_functions(&contract_abi, &mut test_kinds);
    }

    let mut cheatcode_fork = false;

    // Check for cheatcode fork usage
    for test_dir in foundry_config.test.iter() {
        let mut test_dir = repo_setup.working_dir.join(test_dir);
        if !test_dir.exists() {
            // Legacy fallback
            test_dir = repo_setup.working_dir.join("src/test");
            if !test_dir.exists() {
                anyhow::bail!(
                    "Test directory not found in: {}",
                    repo_setup.working_dir.display()
                );
            }
        }
        cheatcode_fork = cheatcode_fork || check_for_fork_cheatcodes(&test_dir)?;
    }

    Ok(RepoAnalysis {
        repo_url: repo_url.to_string(),
        commit: repo_setup.commit,
        test_kinds,
        global_fork: foundry_config.has_eth_rpc_url,
        cheatcode_fork,
    })
}

fn get_foundry_toml(repo_path: &Path) -> anyhow::Result<FoundryConfig> {
    let foundry_toml_path = repo_path.join("foundry.toml");

    if !foundry_toml_path.exists() {
        anyhow::bail!("foundry.toml not found in repository");
    }

    let content = fs::read_to_string(&foundry_toml_path).with_context(|| {
        format!(
            "Failed to read foundry.toml for repo path: {}",
            repo_path.display()
        )
    })?;
    let configs = toml::from_str::<toml::value::Table>(&content)?;
    let profiles = configs
        .get("profile")
        .and_then(|v| v.as_table())
        .with_context(|| {
            format!(
                "No profiles found in foundry.toml: {}",
                foundry_toml_path.display()
            )
        })?;

    let mut test = Vec::new();
    let mut build_profiles = Vec::new();
    let mut has_eth_rpc_url = false;

    // Iterate every profile
    for (profile, value) in profiles {
        match value {
            toml::Value::Table(table) => {
                if let Some(test_config_value) = table.get("test").and_then(|v| v.as_str()) {
                    test.push(test_config_value.to_string());
                }
                if let Some(out_config_value) = table.get("out").and_then(|v| v.as_str()) {
                    build_profiles.push(BuildProfile {
                        out: out_config_value.to_string(),
                        profile: profile.to_string(),
                    })
                } else {
                    build_profiles.push(BuildProfile {
                        out: "out".to_string(),
                        profile: profile.to_string(),
                    })
                }

                has_eth_rpc_url = has_eth_rpc_url || table.contains_key("eth_rpc_url");
            }
            _ => continue,
        }
    }

    Ok(FoundryConfig {
        test,
        build_profiles,
        has_eth_rpc_url,
    })
}

fn find_test_contract_abis(
    repo_path: &Path,
    build_profiles: &[BuildProfile],
) -> anyhow::Result<Vec<JsonAbi>> {
    let mut contracts = Vec::new();

    for build_profile in build_profiles {
        let artifacts_dir = repo_path.join(&build_profile.out);
        if !artifacts_dir.exists() {
            anyhow::bail!("Artifacts directory not found: {}", artifacts_dir.display());
        }

        for entry in WalkDir::new(&artifacts_dir) {
            let entry = entry.with_context(|| {
                format!(
                    "Failed to read artifact entry for repo path: {}",
                    repo_path.display()
                )
            })?;
            let path = entry.path();

            let Some(parent_name) = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|p| p.to_str())
            else {
                continue;
            };

            if !parent_name.ends_with(".sol") {
                continue;
            }

            if path.extension().and_then(|s| s.to_str()) == Some("json")
                && !path.to_string_lossy().ends_with("abi.json")
                && !path.to_string_lossy().ends_with("metadata.json")
            {
                let json = fs::read(path)?;
                let artifact = serde_json::from_slice::<Artifact<'_>>(&json)
                    .with_context(|| format!("Error parsing artifact: '{}'", path.display()))?;
                if is_test_contract(&artifact) {
                    contracts.push(artifact.abi);
                }
            }
        }
    }

    Ok(contracts)
}

fn is_test_contract(artifact: &Artifact<'_>) -> bool {
    // It's an interface
    if artifact.bytecode.object == "0x" {
        return false;
    }

    // Check condition: constructor has no inputs (or no constructor) AND has at
    // least one test function
    let constructor_ok = artifact
        .abi
        .constructor
        .as_ref()
        .map(|c| c.inputs.is_empty())
        .unwrap_or(true);

    let has_test_function = artifact.abi.functions().any(|func| func.is_any_test());

    constructor_ok && has_test_function
}

fn analyze_contract_functions(abi: &JsonAbi, test_kinds: &mut HashMap<TestFunctionKind, bool>) {
    for function in abi.functions() {
        let kind = TestFunctionKind::classify(&function.name, !function.inputs.is_empty());
        match kind {
            TestFunctionKind::Unknown => continue,
            kind => {
                test_kinds.insert(kind, true);
            }
        }
    }
}

fn check_for_fork_cheatcodes(test_dir: &Path) -> anyhow::Result<bool> {
    // Create regex to match fork cheatcodes (case-sensitive)
    let fork_regex = Regex::new(r"vm\.create(?:Select)?Fork\(")?;

    // Walk through all Solidity files in the test directory
    for entry in WalkDir::new(test_dir) {
        let entry = entry.with_context(|| {
            format!(
                "Failed to read directory entry in test dir: {}",
                test_dir.display()
            )
        })?;
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if ext == "sol" {
                    let content = fs::read_to_string(entry.path()).with_context(|| {
                        format!("Failed to read Solidity file: {}", entry.path().display())
                    })?;

                    // Check for fork cheatcodes with regex
                    if fork_regex.is_match(&content) {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(false)
}

fn write_csv_results(analyses: &[RepoAnalysis], output_path: &Path) -> anyhow::Result<()> {
    let file = fs::File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
    let mut writer = Writer::from_writer(file);

    // Write data
    for analysis in analyses {
        let record = CsvRecord::from(analysis.clone());
        writer.serialize(record).with_context(|| {
            format!("Failed to write CSV record for repo: {}", analysis.repo_url)
        })?;
    }

    writer.flush().with_context(|| {
        format!(
            "Failed to flush CSV writer for output file: {}",
            output_path.display()
        )
    })?;
    Ok(())
}

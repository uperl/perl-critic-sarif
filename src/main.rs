use std::{collections::HashMap, path::PathBuf};

use clap::Parser;
use eyre::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_sarif::sarif::{
    ArtifactContentBuilder, ArtifactLocationBuilder, LocationBuilder, MessageBuilder,
    PhysicalLocationBuilder, RegionBuilder, ReportingDescriptor, ReportingDescriptorBuilder,
    Result as SarifResult, ResultBuilder, Run, RunBuilder, Sarif, SarifBuilder, ToolBuilder,
    ToolComponentBuilder, VersionControlDetails, VersionControlDetailsBuilder,
};

/// Convert Perl::Critic JSON violations to SARIF
///
/// Perl::Critic does not ship with a JSON output format, but you can write one trivially
/// with a simple map over the list of violations.
///
/// say encode_json({
///      perl_critic_version => $Perl::Critic::VERSION,
///      violations => [map { violation_to_json($_) } @violations],
/// });
/// sub violation_to_json {
///     my ($violation) = @_;
///     return {
///         filename => $violation->filename,
///         line_number => $violation->line_number,
///         column_number => $violation->column_number,
///         severity => $violation->severity,
///         source => $violation->source,
///         policy => $violation->policy,
///         description => $violation->description,
///         explanation => $violation->explanation,
///         diagnostics => $violation->diagnostics,
///     };
/// }
#[derive(Debug, Parser)]
#[command(version, long_about, verbatim_doc_comment)]
struct Args {
    /// input file; reads from stdin if not provided
    #[clap(short, long)]
    input: Option<PathBuf>,

    /// output file; writes to stdout if not provided
    #[clap(short, long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PerlCriticReport {
    perl_critic_version: String,
    violations: Vec<Violation>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Violation {
    filename: String,
    line_number: u32,
    column_number: u32,
    severity: u8,
    source: String,
    diagnostics: String,
    explanation: String,
    description: String,
    policy: String,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let input: Box<dyn std::io::Read> = match args.input {
        Some(path) => Box::new(std::fs::File::open(path)?),
        None => Box::new(std::io::stdin()),
    };
    let output: Box<dyn std::io::Write> = match args.output {
        Some(path) => Box::new(std::fs::File::create(path)?),
        None => Box::new(std::io::stdout()),
    };

    let report: PerlCriticReport = serde_json::from_reader(input)?;
    let sarif: Sarif = report.try_into()?;
    serde_json::to_writer(output, &sarif)?;

    Ok(())
}

impl PerlCriticReport {
    fn rules(&self) -> Result<Vec<ReportingDescriptor>> {
        let rules = self
            .violations
            .iter()
            .map(|v| {
                let id = policy_to_id(&v.policy);
                let name = policy_to_name(&v.policy);
                let report = ReportingDescriptorBuilder::default()
                    .id(&id)
                    .name(&name)
                    .help_uri(format!(
                        "https://metacpan.org/pod/{policy}",
                        policy = v.policy
                    ))
                    .build()?;
                Ok((id.clone(), report))
            })
            .collect::<Result<HashMap<_, _>>>()?;

        Ok(rules.values().cloned().collect())
    }
}

fn camel_to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}

fn policy_to_id(policy: &str) -> String {
    let id = policy
        .split("::")
        .skip(4)
        .map(camel_to_snake)
        .collect::<Vec<_>>()
        .join("/");
    format!("perl/{id}")
}

fn policy_to_name(policy: &str) -> String {
    policy.split("::").skip(4).collect::<Vec<_>>().join("")
}

impl TryFrom<Violation> for SarifResult {
    type Error = eyre::Report;

    fn try_from(v: Violation) -> Result<Self> {
        let level = match v.severity {
            5 => "error",
            4 => "warning",
            3 => "note",
            _ => "none",
        }
        .to_string();
        let location = LocationBuilder::default()
            .physical_location(
                PhysicalLocationBuilder::default()
                    .artifact_location(
                        ArtifactLocationBuilder::default()
                            .uri(format!("project/{}", v.filename))
                            .uri_base_id("PROJECT")
                            .build()?,
                    )
                    .context_region(
                        RegionBuilder::default()
                            .start_line(v.line_number - 1)
                            .start_column(1)
                            .end_line(v.line_number + 1)
                            .end_column(1)
                            .snippet(ArtifactContentBuilder::default().text(&v.source).build()?)
                            .build()?,
                    )
                    .region(
                        RegionBuilder::default()
                            .start_line(v.line_number)
                            .start_column(v.column_number)
                            .end_line(v.line_number)
                            .end_column(v.source.len() as u32)
                            .snippet(ArtifactContentBuilder::default().text(&v.source).build()?)
                            .build()?,
                    )
                    .build()?,
            )
            .build()?;

        Ok(ResultBuilder::default()
            .message(MessageBuilder::default().text(v.diagnostics).build()?)
            .level(level)
            .rule_id(policy_to_id(&v.policy))
            .locations(vec![location])
            .build()?)
    }
}

impl TryFrom<PerlCriticReport> for Run {
    type Error = eyre::Report;

    fn try_from(report: PerlCriticReport) -> Result<Self> {
        Ok(RunBuilder::default()
            .tool(
                ToolBuilder::default()
                    .driver(
                        ToolComponentBuilder::default()
                            .name("Perl Critic")
                            .full_name("Perl::Critic")
                            .version(&report.perl_critic_version)
                            .information_uri("https://metacpan.org/pod/Perl::Critic")
                            .rules(report.rules()?)
                            .build()?,
                    )
                    .build()?,
            )
            .version_control_provenance(version_control_provenance()?)
            .results(
                report
                    .violations
                    .clone()
                    .into_iter()
                    .map(|v| v.try_into())
                    .collect::<Result<Vec<_>>>()?,
            )
            .build()?)
    }
}

fn version_control_provenance() -> Result<Vec<VersionControlDetails>> {
    let repo = git2::Repository::open_from_env()?;
    let repo_url = git_remote_to_public_url(&repo.config()?.get_string("remote.origin.url")?)?;
    let head = repo.head()?;
    let branch = head.shorthand().unwrap_or("(detached head)");
    let commit = repo.head()?.peel_to_commit()?.id().to_string();
    let details = VersionControlDetailsBuilder::default()
        .repository_uri(repo_url)
        .branch(branch)
        .revision_id(commit)
        .mapped_to( ArtifactLocationBuilder::default()
            .uri("project")
            .uri_base_id("PROJECT")
            .build()?
        )
        .build()?;
    Ok(vec![details])
}

fn git_remote_to_public_url(remote: &str) -> Result<String> {
    let ssh_re = Regex::new(r"(?P<user>[^@]+@)?(?P<host>[^:]+):(?P<repo>.+).git")?;
    let http_re = Regex::new(r"https?://(?P<host>[^/]+)/(?P<repo>.+).git")?;
    let captures = ssh_re
        .captures(remote)
        .or_else(|| http_re.captures(remote))
        .ok_or_else(|| eyre::eyre!("Could not parse remote"))?;
    let caps = (captures.name("host"), captures.name("repo"));
    if let (Some(host), Some(repo)) = caps {
        Ok(format!(
            "https://{host}/{repo}",
            host = host.as_str(),
            repo = repo.as_str()
        ))
    } else {
        Err(eyre::eyre!("Could not parse remote"))
    }
}

impl TryFrom<PerlCriticReport> for Sarif {
    type Error = eyre::Report;

    fn try_from(report: PerlCriticReport) -> Result<Self> {
        Ok(SarifBuilder::default()
            .runs(vec![report.try_into()?])
            .schema("https://raw.githubusercontent.com/oasis-tcs/sarif-spec/master/Schemata/sarif-schema-2.1.0.json".to_string())
            .version("2.1.0")
            .build()
            ?)
    }
}

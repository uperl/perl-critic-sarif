perl-critic-sarif
=================

This is a rust program that can used to translate
[Perl::Critic](https://metacpan.org/dist/Perl-Critic) violations into the SARIF
format. This allows Perl projects to participate in GitHub Advanced Security
scanning.


Usage
-----

```
Usage: perl-critic-sarif [OPTIONS]

Options:
  -i, --input <INPUT>
          input file; reads from stdin if not provided

  -o, --output <OUTPUT>
          output file; writes to stdout if not provided

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```


Mapping Severity to Level
-------------------------

The mapping of perl critic's `severity` to SARIF's `level` is naive.
Severities 1 and 2 are always ignored - mapped to the "none" level in SARIF.
Severity 3 maps to "note", Severity 4 to "warning", and Severity 5 to "error".

This may not be entirely correct per [SARIF 3.27.10](https://docs.oasis-open.org/sarif/sarif/v2.0/csprd02/sarif-v2.0-csprd02.html#_Toc10127839)
and some way of controlling how these are mapped me be added in the future.


Intermediate JSON Format
------------------------

Unfortunately, Perl::Critic does not have a JSON output format.  This program
expects to receive a JSON file following this basic structure

```json
{
    "perl_critic_version": "1.50",
    "violations": [
        {
            "filename": "lib/My/Module.pm",
            "line_number": 1,
            "column_number": 1,
            "severity": 5,
            "source": "part of the code that triggered the violation",
            "policy": "Perl::Critic::Policy::Something",
            "description": "description",
            "explanation": "explanation",
            "diagnostics": "diagnostics"
        }
    ]
}
```

Example Test File
-----------------


Using
[Test2::Tools::PerlCritic](https://metacpan.org/pod/Test2::Tools::PerlCritic)
it is possible to have a unit test the passes/fails that writes out a JSON file
in the format this program expects:

```perl
#!/usr/bin/env perl
use Test2::V0;
use File::Glob qw( bsd_glob );
use Perl::Critic;
use Perl::Critic::Utils qw( all_perl_files );
use Test2::Tools::PerlCritic;
use Cpanel::JSON::XS qw( encode_json );
use experimental 'signatures';

my @bin = all_perl_files 'bin';

note "checking bin: $_" for @bin;

my $critic = Perl::Critic->new(-profile => '.perlcriticrc');

my $test_critic = Test2::Tools::PerlCritic->new(
    {
        critic => $critic,
        files  => [ 'lib', 't', @bin ],
    }
);

my @all_violations;

$test_critic->add_hook(
    violations => sub ( $test_critic, @violations ) {
        my $count = @violations;
        push @all_violations, @violations;
        pass;
    }
);

$test_critic->perl_critic_ok;

open my $fh, '>', 'perl-critic.json' or die "Can't open perl-critic.json: $!";
print $fh encode_json(
    {
        perl_critic_version => $Perl::Critic::VERSION,
        violations          => [ map { violation_to_json($_) } @all_violations ],
    },
);
close $fh;

done_testing;

sub violation_to_json {
    my ($violation) = @_;
    return {
        filename      => $violation->filename,
        line_number   => $violation->line_number,
        column_number => $violation->column_number,
        severity      => $violation->severity,
        source        => $violation->source,
        policy        => $violation->policy,
        description   => $violation->description,
        explanation   => $violation->explanation,
        diagnostics   => $violation->diagnostics,
    };
}


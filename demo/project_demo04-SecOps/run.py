from itertools import islice
import os
import sys
import subprocess

from dbsp import DBSPPipelineConfig
from dbsp import CsvInputFormatConfig, CsvOutputFormatConfig
from dbsp import KafkaInputConfig
from dbsp import KafkaOutputConfig

# Import
sys.path.append(os.path.join(os.path.dirname(os.path.dirname(__file__)), ".."))
from demo import *

SCRIPT_DIR = os.path.join(os.path.dirname(__file__))


def prepare(args=[2000000]):
    assert len(args) == 1, "Expected one '--prepare-args' argument for num_pipelines"
    num_pipelines = args[0]
    cmd = ["cargo", "run", "--release", "--", "%s" % num_pipelines]
    # Override --release if RUST_BUILD_PROFILE is set
    if "RUST_BUILD_PROFILE" in os.environ:
        cmd[2] = os.environ["RUST_BUILD_PROFILE"]
    subprocess.run(cmd, cwd=os.path.join(SCRIPT_DIR, "simulator"))

    from plumbum.cmd import rpk
    rpk['topic', 'delete', 'secops_vulnerability_stats']()
    rpk['topic', 'create', 'secops_vulnerability_stats',
        '-c', 'retention.ms=-1', '-c', 'retention.bytes=-1']()

def make_config(project):
    config = DBSPPipelineConfig(project, 8, "SecOps Pipeline")

    config.add_kafka_input(
        name="secops_pipeline_sources",
        stream="PIPELINE_SOURCES",
        config=KafkaInputConfig.from_dict(
            {"topics": ["secops_pipeline_sources"], "auto.offset.reset": "earliest"}
        ),
        format=CsvInputFormatConfig(),
    )
    config.add_kafka_input(
        name="secops_artifact",
        stream="ARTIFACT",
        config=KafkaInputConfig.from_dict(
            {"topics": ["secops_artifact"], "auto.offset.reset": "earliest"}
        ),
        format=CsvInputFormatConfig(),
    )
    config.add_kafka_input(
        name="secops_vulnerability",
        stream="VULNERABILITY",
        config=KafkaInputConfig.from_dict(
            {"topics": ["secops_vulnerability"], "auto.offset.reset": "earliest"}
        ),
        format=CsvInputFormatConfig(),
    )
    config.add_kafka_input(
        name="secops_cluster",
        stream="K8SCLUSTER",
        config=KafkaInputConfig.from_dict(
            {"topics": ["secops_cluster"], "auto.offset.reset": "earliest"}
        ),
        format=CsvInputFormatConfig(),
    )
    config.add_kafka_input(
        name="secops_k8sobject",
        stream="K8SOBJECT",
        config=KafkaInputConfig.from_dict(
            {"topics": ["secops_k8sobject"], "auto.offset.reset": "earliest"}
        ),
        format=CsvInputFormatConfig(),
    )
    config.add_kafka_output(
        name='secops_vulnerability_stats',
        stream='K8SCLUSTER_VULNERABILITY_STATS',
        config=KafkaOutputConfig.from_dict(
                {'topic': 'secops_vulnerability_stats'}),
        format=CsvOutputFormatConfig(),
    )

    config.save()
    return config


if __name__ == "__main__":
    run_demo(
        "SecOps demo", os.path.join(SCRIPT_DIR, "project.sql"), make_config, prepare
    )

from typing import TYPE_CHECKING, Any, Dict, List, Type, TypeVar

import attr

if TYPE_CHECKING:
    from ..models.connector_descr import ConnectorDescr
    from ..models.pipeline_descr import PipelineDescr
    from ..models.program_descr import ProgramDescr


T = TypeVar("T", bound="PipelineRevision")


@attr.s(auto_attribs=True)
class PipelineRevision:
    """A pipeline revision is a versioned, immutable configuration struct that
    contains all information necessary to run a pipeline.

        Attributes:
            code (str): The versioned SQL code.
            config (str): The generated TOML config for the pipeline.
            connectors (List['ConnectorDescr']): The versioned connectors.
            pipeline (PipelineDescr): Pipeline descriptor.
            program (ProgramDescr): Program descriptor.
            revision (str): Revision number.
    """

    code: str
    config: str
    connectors: List["ConnectorDescr"]
    pipeline: "PipelineDescr"
    program: "ProgramDescr"
    revision: str
    additional_properties: Dict[str, Any] = attr.ib(init=False, factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        code = self.code
        config = self.config
        connectors = []
        for connectors_item_data in self.connectors:
            connectors_item = connectors_item_data.to_dict()

            connectors.append(connectors_item)

        pipeline = self.pipeline.to_dict()

        program = self.program.to_dict()

        revision = self.revision

        field_dict: Dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code": code,
                "config": config,
                "connectors": connectors,
                "pipeline": pipeline,
                "program": program,
                "revision": revision,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: Type[T], src_dict: Dict[str, Any]) -> T:
        from ..models.connector_descr import ConnectorDescr
        from ..models.pipeline_descr import PipelineDescr
        from ..models.program_descr import ProgramDescr

        d = src_dict.copy()
        code = d.pop("code")

        config = d.pop("config")

        connectors = []
        _connectors = d.pop("connectors")
        for connectors_item_data in _connectors:
            connectors_item = ConnectorDescr.from_dict(connectors_item_data)

            connectors.append(connectors_item)

        pipeline = PipelineDescr.from_dict(d.pop("pipeline"))

        program = ProgramDescr.from_dict(d.pop("program"))

        revision = d.pop("revision")

        pipeline_revision = cls(
            code=code,
            config=config,
            connectors=connectors,
            pipeline=pipeline,
            program=program,
            revision=revision,
        )

        pipeline_revision.additional_properties = d
        return pipeline_revision

    @property
    def additional_keys(self) -> List[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties

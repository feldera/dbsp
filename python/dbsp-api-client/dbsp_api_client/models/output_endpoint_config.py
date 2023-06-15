from typing import TYPE_CHECKING, Any, Dict, List, Type, TypeVar, Union

import attr

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.format_config import FormatConfig
    from ..models.transport_config import TransportConfig


T = TypeVar("T", bound="OutputEndpointConfig")


@attr.s(auto_attribs=True)
class OutputEndpointConfig:
    """
    Attributes:
        format_ (FormatConfig): Data format specification used to parse raw data received from the
            endpoint or to encode data sent to the endpoint.
        stream (str): The name of the output stream of the circuit that this endpoint is
            connected to.
        transport (TransportConfig): Transport endpoint configuration.
        max_buffered_records (Union[Unset, int]): Backpressure threshold.

            The default is 1 million.
    """

    format_: "FormatConfig"
    stream: str
    transport: "TransportConfig"
    max_buffered_records: Union[Unset, int] = UNSET
    additional_properties: Dict[str, Any] = attr.ib(init=False, factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        format_ = self.format_.to_dict()

        stream = self.stream
        transport = self.transport.to_dict()

        max_buffered_records = self.max_buffered_records

        field_dict: Dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "format": format_,
                "stream": stream,
                "transport": transport,
            }
        )
        if max_buffered_records is not UNSET:
            field_dict["max_buffered_records"] = max_buffered_records

        return field_dict

    @classmethod
    def from_dict(cls: Type[T], src_dict: Dict[str, Any]) -> T:
        from ..models.format_config import FormatConfig
        from ..models.transport_config import TransportConfig

        d = src_dict.copy()
        format_ = FormatConfig.from_dict(d.pop("format"))

        stream = d.pop("stream")

        transport = TransportConfig.from_dict(d.pop("transport"))

        max_buffered_records = d.pop("max_buffered_records", UNSET)

        output_endpoint_config = cls(
            format_=format_,
            stream=stream,
            transport=transport,
            max_buffered_records=max_buffered_records,
        )

        output_endpoint_config.additional_properties = d
        return output_endpoint_config

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

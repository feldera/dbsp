from http import HTTPStatus
from typing import Any, Dict, Optional, Union

import httpx

from ... import errors
from ...client import Client
from ...models.error_response import ErrorResponse
from ...models.pipeline_revision import PipelineRevision
from ...types import Response


def _get_kwargs(
    pipeline_id: str,
    *,
    client: Client,
) -> Dict[str, Any]:
    url = "{}/pipelines/{pipeline_id}/committed".format(client.base_url, pipeline_id=pipeline_id)

    headers: Dict[str, str] = client.get_headers()
    cookies: Dict[str, Any] = client.get_cookies()

    return {
        "method": "get",
        "url": url,
        "headers": headers,
        "cookies": cookies,
        "timeout": client.get_timeout(),
        "follow_redirects": client.follow_redirects,
    }


def _parse_response(
    *, client: Client, response: httpx.Response
) -> Optional[Union[ErrorResponse, Optional[PipelineRevision]]]:
    if response.status_code == HTTPStatus.OK:
        _response_200 = response.json()
        response_200: Optional[PipelineRevision]
        if _response_200 is None:
            response_200 = None
        else:
            response_200 = PipelineRevision.from_dict(_response_200)

        return response_200
    if response.status_code == HTTPStatus.NOT_FOUND:
        response_404 = ErrorResponse.from_dict(response.json())

        return response_404
    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: Client, response: httpx.Response
) -> Response[Union[ErrorResponse, Optional[PipelineRevision]]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    pipeline_id: str,
    *,
    client: Client,
) -> Response[Union[ErrorResponse, Optional[PipelineRevision]]]:
    """Return the last committed (and running, if pipeline is started)

     Return the last committed (and running, if pipeline is started)
    configuration of the pipeline.

    Args:
        pipeline_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ErrorResponse, Optional[PipelineRevision]]]
    """

    kwargs = _get_kwargs(
        pipeline_id=pipeline_id,
        client=client,
    )

    response = httpx.request(
        verify=client.verify_ssl,
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    pipeline_id: str,
    *,
    client: Client,
) -> Optional[Union[ErrorResponse, Optional[PipelineRevision]]]:
    """Return the last committed (and running, if pipeline is started)

     Return the last committed (and running, if pipeline is started)
    configuration of the pipeline.

    Args:
        pipeline_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ErrorResponse, Optional[PipelineRevision]]
    """

    return sync_detailed(
        pipeline_id=pipeline_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    pipeline_id: str,
    *,
    client: Client,
) -> Response[Union[ErrorResponse, Optional[PipelineRevision]]]:
    """Return the last committed (and running, if pipeline is started)

     Return the last committed (and running, if pipeline is started)
    configuration of the pipeline.

    Args:
        pipeline_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Union[ErrorResponse, Optional[PipelineRevision]]]
    """

    kwargs = _get_kwargs(
        pipeline_id=pipeline_id,
        client=client,
    )

    async with httpx.AsyncClient(verify=client.verify_ssl) as _client:
        response = await _client.request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    pipeline_id: str,
    *,
    client: Client,
) -> Optional[Union[ErrorResponse, Optional[PipelineRevision]]]:
    """Return the last committed (and running, if pipeline is started)

     Return the last committed (and running, if pipeline is started)
    configuration of the pipeline.

    Args:
        pipeline_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Union[ErrorResponse, Optional[PipelineRevision]]
    """

    return (
        await asyncio_detailed(
            pipeline_id=pipeline_id,
            client=client,
        )
    ).parsed

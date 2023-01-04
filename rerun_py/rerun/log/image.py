from typing import Optional

import numpy as np
import numpy.typing as npt
from rerun.log.error_utils import _send_warning
from rerun.log.tensor import Tensor, _log_tensor, _to_numpy

from rerun import bindings

__all__ = [
    "log_image",
    "log_depth_image",
    "log_segmentation_image",
]


def log_image(
    obj_path: str,
    image: Tensor,
    *,
    timeless: bool = False,
) -> None:
    """
    Log a gray or color image.

    The image should either have 1, 3 or 4 channels (gray, RGB or RGBA).

    Supported `dtype`s:
    * uint8: color components should be in 0-255 sRGB gamma space, except for alpha which should be in 0-255 linear
    space.
    * uint16: color components should be in 0-65535 sRGB gamma space, except for alpha which should be in 0-65535
    linear space.
    * float32/float64: all color components should be in 0-1 linear space.

    """
    image = _to_numpy(image)

    shape = image.shape
    non_empty_dims = [d for d in shape if d != 1]
    num_non_empty_dims = len(non_empty_dims)

    interpretable_as_image = False
    # Catch some errors early:
    if num_non_empty_dims < 2 or 3 < num_non_empty_dims:
        _send_warning(f"Expected image, got array of shape {shape}", 1)
        interpretable_as_image = False

    if num_non_empty_dims == 3:
        depth = shape[-1]
        if depth not in (1, 3, 4):
            _send_warning(
                f"Expected image depth of 1 (gray), 3 (RGB) or 4 (RGBA). Instead got array of shape {shape}", 1
            )
            interpretable_as_image = False

    # TODO(#672): Don't squeeze once the image view can handle extra empty dimensions
    if interpretable_as_image and num_non_empty_dims != len(shape):
        image = np.squeeze(image)

    _log_tensor(obj_path, image, timeless=timeless)


def log_depth_image(
    obj_path: str,
    image: Tensor,
    *,
    meter: Optional[float] = None,
    timeless: bool = False,
) -> None:
    """
    Log a depth image.

    The image must be a 2D array. Supported `dtype`:s are: uint8, uint16, float32, float64

    meter: How long is a meter in the given dtype?
           For instance: with uint16, perhaps meter=1000 which would mean
           you have millimeter precision and a range of up to ~65 meters (2^16 / 1000).

    """
    image = _to_numpy(image)

    # TODO(#635): Remove when issue with displaying f64 depth images is fixed.
    if image.dtype == np.float64:
        image = image.astype(np.float32)

    shape = image.shape
    non_empty_dims = [d for d in shape if d != 1]
    num_non_empty_dims = len(non_empty_dims)

    # Catch some errors early:
    if num_non_empty_dims != 2:
        _send_warning(f"Expected 2D depth image, got array of shape {shape}", 1)
        _log_tensor(obj_path, image, timeless=timeless)
    else:
        # TODO(#672): Don't squeeze once the image view can handle extra empty dimensions.
        if num_non_empty_dims != len(shape):
            image = np.squeeze(image)
        _log_tensor(obj_path, image, meter=meter, timeless=timeless)


def log_segmentation_image(
    obj_path: str,
    image: npt.ArrayLike,
    *,
    timeless: bool = False,
) -> None:
    """
    Log an image made up of integer class-ids.

    The image should have 1 channel, i.e. be either `H x W` or `H x W x 1`.
    """
    image = np.array(image, dtype=np.uint16, copy=False)
    non_empty_dims = [d for d in image.shape if d != 1]
    num_non_empty_dims = len(non_empty_dims)

    # Catch some errors early:
    if num_non_empty_dims != 2:
        _send_warning(
            f"Expected single channel image, got array of shape {image.shape}. Can't interpret as segmentation image.",
            1,
        )
        _log_tensor(
            obj_path,
            tensor=image,
            timeless=timeless,
        )
    else:
        # TODO(#672): Don't squeeze once the image view can handle extra empty dimensions.
        if num_non_empty_dims != len(image.shape):
            image = np.squeeze(image)
        _log_tensor(
            obj_path,
            tensor=image,
            meaning=bindings.TensorDataMeaning.ClassId,
            timeless=timeless,
        )
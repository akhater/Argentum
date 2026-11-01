/**
 * Who Argentum is built on. Ours.
 *
 * Grouped rather than listed flat, because the list will grow unevenly. Two
 * projects are the reason this one exists; anything harvested later is a
 * different kind of debt and belongs under its own heading rather than diluting
 * theirs.
 *
 * WHAT CAME WITH RAPIDRAW
 *
 * The models and libraries RapidRAW brought have a group of their own, last,
 * headed as inherited: RapidRAW's own list of them is no longer shown since its
 * acknowledgements left the General tab, and they ship here just the same.
 * Under any other heading, a dependency earns an entry when *Argentum* brought
 * it in. CREDITS.md says the same things, in the same groups.
 */

export interface Credit {
  name: string;
  /** Left out where the code cites no one upstream, rather than guessing one. */
  href?: string;
  what: string;
}

export interface CreditGroup {
  heading: string;
  blurb?: string;
  entries: Credit[];
}

export const CREDITS: CreditGroup[] = [
  {
    heading: 'Built on',
    blurb: 'Argentum is not written from scratch. These two are the reason it exists at all.',
    entries: [
      {
        name: 'RapidRAW',
        href: 'https://github.com/CyberTimon/RapidRAW',
        what:
          'By Timon Käch. Argentum is a fork of it. The interface, the GPU pipeline, ' +
          'the catalogue and very nearly all of the application are his work, along ' +
          'with the libraries and models he credits inside it.',
      },
      {
        name: 'darktable',
        href: 'https://github.com/darktable-org/darktable',
        what:
          'The reference Argentum is measured against, and the source of its automatic ' +
          'white balance, of the raw-level behaviour its decoder gets right, and of the ' +
          'unsharp mask in Sharpening (its sharpen module).',
      },
    ],
  },
  {
    heading: 'Taken from pull requests',
    blurb:
      "Work by RapidRAW's contributors that Argentum took from their pull " +
      'requests. Each is marked in the source between `// upstream #NNNN` and ' +
      '`// end upstream #NNNN`, so it stays obvious whose it is.',
    entries: [
      {
        name: 'dimafa — #1466, high-precision export',
        href: 'https://github.com/CyberTimon/RapidRAW/pull/1466',
        what:
          'Argentum’s 16-bit TIFF export is built on their idea: make the export ' +
          'pipeline by rewriting the storage format in the shader source, and switch ' +
          'the 8-bit dither off with a pipeline constant. The idea is theirs; the code ' +
          'was written here, and the 32-bit target and the tests are ours.',
      },
      {
        name: '#1307, the AI patch cache key',
        href: 'https://github.com/CyberTimon/RapidRAW/pull/1307',
        what:
          'Two AI patches of the same length no longer share a cache entry, so the ' +
          'wrong one is not rendered from cache.',
      },
      {
        name: '#1633, the sRGB exponent',
        href: 'https://github.com/CyberTimon/RapidRAW/pull/1633',
        what: 'sRGB decoding uses an exponent of 2.4, which is what the standard says.',
      },
    ],
  },
  {
    heading: 'Sharpening',
    entries: [
      {
        name: 'RawTherapee',
        href: 'https://github.com/RawTherapee/RawTherapee',
        what:
          "Capture sharpening is RawTherapee's, by Ingo Weyrich: the Richardson-Lucy " +
          'deconvolution, the radius it reads from the RAW, and the contrast mask that ' +
          'keeps it off flat areas and noise, with its automatic threshold. Ported to ' +
          'the GPU from rtengine/capturesharpening.cc and rt_algo.cc.',
      },
    ],
  },
  {
    heading: 'RAW decoding',
    entries: [
      {
        name: 'FlashRaw',
        href: 'https://github.com/ybouane/flashraw',
        what:
          'With Settings > Processing > RAW decoder on FlashRaw, it unpacks the pixels of Canon, ' +
          'Fujifilm, Olympus, Panasonic and Pentax RAW files, faster, inside rawler’s reading of the ' +
          'file: only on the formats where its pixels were measured identical to rawler’s. Used as a ' +
          'library. MIT or Apache-2.0.',
      },
    ],
  },
  {
    heading: 'RAW rendering',
    entries: [
      {
        name: 'RAWmakase',
        href: 'https://github.com/pch/rawmakase',
        what:
          'DNG Standard, the look every RAW now starts in, follows RAWmakase’s default rendering: ' +
          'each camera’s exposure offset, from RAWmakase’s camera table, which its contributors ' +
          'measured against Camera Raw (280 bodies across 10 makes); Adobe’s default tone curve, ' +
          'applied so hues hold; and Lightroom’s default sharpening, matched against RAWmakase’s ' +
          'output. B&W’s black and white mix is RAWmakase’s too: the grey Camera Raw makes of each ' +
          'colour, measured on a colour chart; and Color Grading for new edits and Looks is ' +
          'RAWmakase’s measured copy of Lightroom’s. The camera’s own lens correction is read as ' +
          'RAWmakase reads it: the distortion, colour fringing and vignetting tables each make ' +
          'stores in its RAW files, which of them Lightroom applies, the optical centre they turn ' +
          'about and how Lightroom frames the corrected photo. Some RAW files are read as ' +
          'RAWmakase measured them: the Canon EOS R8 Mark II’s colour data, and a few cameras’ colour ' +
          'matrices and white levels, and the names Adobe profiles some cameras under (the Lumix ' +
          'G97 as the G99). Noise below black is clipped where RAWmakase clips it, after ' +
          'demosaicing. The presets that come with Argentum (Color, Creative, B&W, ' +
          'Curve, Grain and Vignetting, 26 Lightroom XMP files) are RAWmakase’s, shipped ' +
          'unchanged. RAWmakase is also where the comparison started. MIT.',
      },
      {
        name: 'RawTherapee',
        href: 'https://github.com/RawTherapee/RawTherapee',
        what:
          'DNG Standard’s tone curve is applied as RawTherapee applies a camera profile’s: to the ' +
          'largest and smallest channel, with the middle one kept between them ' +
          '(rtengine/curves.h). The curve itself is taken from RawTherapee’s rtengine/dcp.cc.',
      },
      {
        name: 'Adobe DNG SDK',
        href: 'https://helpx.adobe.com/camera-raw/digital-negative.html',
        what:
          'The default tone curve DNG Standard applies is the DNG SDK’s, copyright Adobe Inc., ' +
          'under the SDK’s licence. Argentum is not affiliated with or endorsed by Adobe.',
      },
    ],
  },
  {
    heading: 'AI masks',
    entries: [
      {
        name: 'RAWmakase',
        href: 'https://github.com/pch/rawmakase',
        what:
          'Subject, Sky and Background are RAWmakase’s selection code (crates/rawmakase-inference ' +
          '@ 7c3c81a), taken whole: DETR finds the people, animals and sky, SAM 2.1 draws their ' +
          'outlines, and its edge refinement and its rule for where a sky stops are kept as ' +
          'written. MIT.',
      },
      {
        name: 'SAM 2.1 (Meta AI)',
        href: 'https://github.com/facebookresearch/sam2',
        what:
          'Segment Anything Model 2.1, Hiera small, draws the outline of each selection. ' +
          'Apache-2.0; ONNX export by the Hugging Face onnx-community.',
      },
      {
        name: 'DETR (Facebook AI Research)',
        href: 'https://github.com/facebookresearch/detr',
        what:
          'DETR ResNet-50 panoptic, trained on COCO, says where a photo’s people, animals and sky ' +
          'are. Apache-2.0; ONNX export by Xenova.',
      },
      {
        name: 'EfficientSAM3',
        href: 'https://github.com/SimonZeng7108/efficientsam3',
        what:
          'The Text mask finds what is typed with EfficientSAM3 EV-M, an image encoder, a text ' +
          'encoder and a decoder distilled from Meta’s SAM 3, reading the words through CLIP’s ' +
          'word list. Apache-2.0 as its authors publish it; SAM 3 itself is under Meta’s SAM ' +
          'License. The ONNX files and the code that runs them are RapidRAW 1.6.7’s. It also finds ' +
          'the faces for the Face mask.',
      },
      {
        name: 'MediaPipe (Google)',
        href: 'https://github.com/google-ai-edge/mediapipe',
        what:
          'The Face mask draws each face’s parts with MediaPipe’s face landmarks (478 points) and ' +
          'its selfie multiclass segmenter (hair, face and body skin). Apache-2.0. The ONNX files ' +
          'and the code that runs them are RapidRAW 1.6.7’s.',
      },
    ],
  },
  {
    heading: 'Camera profiles',
    blurb:
      'Argentum reads camera profiles but ships none — the free collections are ' +
      "published with their authors' individual permission rather than under a " +
      'licence that lets anyone else redistribute them.',
    entries: [
      {
        name: 'RawTherapee',
        href: 'https://github.com/RawTherapee/RawTherapee',
        what:
          'Publishes a collection of hand-made DCP camera profiles, contributed by ' +
          'their authors. When Argentum fetches a profile for your camera, it ' +
          'comes from there — from the project that published it, not from us.',
      },
    ],
  },
  {
    heading: 'AI enlargement and restoration',
    blurb: 'Local models, downloaded on first use and verified before inference.',
    entries: [
      {
        name: 'Real-ESRGAN',
        href: 'https://github.com/xinntao/Real-ESRGAN',
        what:
          'The open-source models behind Argentum’s local 2x and 4x photo enlargement. ' +
          'The original project is released under the BSD-3-Clause licence.',
      },
      {
        name: 'OmniLens',
        href: 'https://github.com/zju-jiangqi/OmniLens',
        what:
          'Restore’s model, by Qi Jiang and colleagues at Zhejiang University: a SwinIR network ' +
          '(Jingyun Liang et al.) trained on thousands of simulated lens designs, so it undoes the ' +
          'softness of a lens it has never seen. Apache-2.0.',
      },
      {
        name: 'ONNX Runtime with DirectML',
        href: 'https://github.com/microsoft/onnxruntime',
        what:
          'Microsoft’s DirectML build of the runtime RapidRAW already uses. On Windows it is ' +
          'downloaded the first time you enlarge or restore a photo, and runs it on the ' +
          'graphics card. MIT licence.',
      },
    ],
  },
  {
    heading: 'Inherited with RapidRAW',
    blurb:
      'Argentum ships all of RapidRAW, so everything RapidRAW depends on, Argentum depends ' +
      'on too. These are its credits, repeated because they are just as true of this app.',
    entries: [
      {
        name: 'rawler',
        href: 'https://github.com/dnglab/dnglab',
        what: 'RAW decoding. The foundation everything else stands on.',
      },
      {
        name: 'lensfun',
        href: 'https://lensfun.github.io/',
        what: 'The lens correction library and its camera and lens database.',
      },
      {
        name: 'Segment Anything (SAM)',
        href: 'https://github.com/facebookresearch/segment-anything',
        what: 'The model that cuts out what a box is drawn around, for Quick Erase and Inpainting.',
      },
      {
        name: 'U²-Net',
        href: 'https://github.com/xuebinqin/U-2-Net',
        what: 'The architecture behind Foreground detection.',
      },
      {
        name: 'LaMa',
        href: 'https://github.com/advimman/lama',
        what: 'Inpainting: content-aware fill and object removal.',
      },
      {
        name: 'Depth Anything',
        what: 'Depth estimation, which drives the Depth mask, Lens blur and Fog.',
      },
      {
        name: 'NIND',
        what: 'The models behind AI noise reduction.',
      },
      {
        name: 'NegPy',
        href: 'https://github.com/marcinz606/NegPy',
        what: 'The approach the film-negative conversion is based on.',
      },
      {
        name: 'Spektrafilm — Andrea Volpato',
        what: 'The spectrally-based film emulation LUTs. Licensed CC BY-SA 4.0.',
      },
      {
        name: 'libgphoto2',
        href: 'https://github.com/gphoto/libgphoto2',
        what: 'Camera communication for tethering, only in builds from source for macOS and Linux.',
      },
    ],
  },
];

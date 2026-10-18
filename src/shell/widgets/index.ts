// The widget for each setting kind the contract has. Curve and colour come
// later, registered the same way.

import { registerWidget } from '../panels/registry';
import { Choice } from './Choice';
import { Slider } from './Slider';
import { Toggle } from './Toggle';

registerWidget('float', Slider);
registerWidget('int', Slider);
registerWidget('bool', Toggle);
registerWidget('choice', Choice);

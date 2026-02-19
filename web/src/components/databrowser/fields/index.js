import ReadOnlyField from './ReadOnlyField';
import TextField from './TextField';
import NumberField from './NumberField';
import CurrencyField from './CurrencyField';
import PercentField from './PercentField';
import BooleanField from './BooleanField';
import DateTimeField from './DateTimeField';
import EmailField from './EmailField';
import UrlField from './UrlField';
import PhoneField from './PhoneField';
import ChoiceField from './ChoiceField';
import MultiChoiceField from './MultiChoiceField';
import JsonField from './JsonField';
import LookupField from './LookupField';

const FIELD_MAP = {
  text: TextField,
  number: NumberField,
  decimal: NumberField,
  currency: CurrencyField,
  percent: PercentField,
  boolean: BooleanField,
  date: DateTimeField,
  time: DateTimeField,
  date_time: DateTimeField,
  email: EmailField,
  url: UrlField,
  phone: PhoneField,
  choice: ChoiceField,
  multi_choice: MultiChoiceField,
  json: JsonField,
  lookup: LookupField,
  auto_increment: ReadOnlyField,
  uuid: TextField,
  duration: TextField,
  formula: ReadOnlyField,
};

export function getFieldComponent(fieldType) {
  return FIELD_MAP[fieldType] || TextField;
}

export { default as ReadOnlyField } from './ReadOnlyField';
export { default as TextField } from './TextField';
export { default as NumberField } from './NumberField';
export { default as CurrencyField } from './CurrencyField';
export { default as PercentField } from './PercentField';
export { default as BooleanField } from './BooleanField';
export { default as DateTimeField } from './DateTimeField';
export { default as EmailField } from './EmailField';
export { default as UrlField } from './UrlField';
export { default as PhoneField } from './PhoneField';
export { default as ChoiceField } from './ChoiceField';
export { default as MultiChoiceField } from './MultiChoiceField';
export { default as JsonField } from './JsonField';
export { default as LookupField } from './LookupField';

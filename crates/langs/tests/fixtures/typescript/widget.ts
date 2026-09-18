import { Alpha } from "./alpha";
import Default, { beta as renamed } from "./mixed";

export interface Shape {
  width: number;
}

export type Alias = Shape;

const makeWidget = (base: Alpha): Alias => {
  return build(base);
};

export class Widget extends BaseWidget implements Shape {
  size: number = 1;
  render(): Alias {
    return makeWidget(this.size);
  }
}

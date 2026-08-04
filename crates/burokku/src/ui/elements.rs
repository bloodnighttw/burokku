use crate::ui::elements::styles::{flex::FlexStyle, grid::GridStyle};

pub mod styles;

// represent the layout tree of window app
pub enum Elements {
    // the root of app, its children should only accept [`Self::Window`]
    // and if user pass other element to App, it should panic.
    App,
    
    // the top of window, we can use it to create multiple window 
    // in js.
    // 
    // currently, we only supported one <window> inside <app>, in future, we will
    // support mulitple window.
    // 
    // note it shouldn't nested Window inside Window, if user do so, it should panic
    Window,
    
    // the block layout just like css
    Div,
    
    // the flex layout
    Flex {
        style: Box<FlexStyle>,
    },
    
    // the grid layout
    Grid {
        style: Box<GridStyle>
    },
    // the text element
    Text,    
}

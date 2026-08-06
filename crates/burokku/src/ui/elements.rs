use self::styles::{
    flex::{FlexItemStyle, FlexStyle},
    grid::{GridItemStyle, GridStyle},
};

mod iter;

pub use iter::ElementsIter;
pub mod styles;

// represent the layout tree of window app
pub enum Elements {
    // the root of app, its children should only accept [`Self::Window`]
    // and if user pass other element to App, it should ignore.
    App {
        // should only have Window
        children: Vec<Elements>,
    },

    // the top of window, we can use it to create multiple window
    // in js.
    //
    // currently, we only supported one <window> inside <app>, in future, we will
    // support mulitple window.
    //
    // note it shouldn't nested Window inside Window, if user do so, it should ignore
    Window {
        // should only have Div/Flex/Grid/Text
        children: Vec<Elements>,
    },

    // the block layout <div>
    Div {
        flex_item_style: Box<FlexItemStyle>,
        grid_item_style: Box<GridItemStyle>,
        // should only have Div/Flex/Grid/Text
        children: Vec<Elements>,
    },

    // the flex layout element <flex>
    Flex {
        style: Box<FlexStyle>,
        flex_item_style: Box<FlexItemStyle>,
        grid_item_style: Box<GridItemStyle>,
        // should only have Div/Flex/Grid/Text
        children: Vec<Elements>,
    },

    // the grid layout <grid>
    Grid {
        style: Box<GridStyle>,
        flex_item_style: Box<FlexItemStyle>,
        grid_item_style: Box<GridItemStyle>,
        // should only have Div/Flex/Grid/Text
        children: Vec<Elements>,
    },
    // the text element <text>
    Text {
        flex_item_style: Box<FlexItemStyle>,
        grid_item_style: Box<GridItemStyle>,
        // should only have Self::_String or Self::Text
        children: Vec<Elements>,
    },
    // internel element, it is to allow somethings like
    // <text> hi! I'm <text style={{...}}/> Ben </text> <text>
    // it should not being used by user.
    _String {
        string: String,
    },
}

impl Elements {
    /// Iterates over this element and its valid descendants in tree order.
    pub fn iter(&self) -> ElementsIter<'_> {
        ElementsIter::new(self)
    }

    pub fn children(&self) -> Option<&Vec<Elements>> {
        match self {
            Self::App { children }
            | Self::Window { children }
            | Self::Div { children, .. }
            | Self::Flex { children, .. }
            | Self::Grid { children, .. }
            | Self::Text { children, .. } => Some(children),
            Self::_String { .. } => None,
        }
    }
}

impl<'a> IntoIterator for &'a Elements {
    type Item = &'a Elements;
    type IntoIter = ElementsIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
